#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_rp::{
    gpio::{Input, Output, Level, Pull},
    pwm::{Pwm, Config as PwmConfig, SetDutyCycle},
    i2c::{self, I2c},
    init,
};
use embassy_time::{Timer, Instant, Delay, Duration};
use fixed::traits::ToFixed;
use hd44780_driver::{HD44780, Display};
use heapless::String;
use core::fmt::Write;
use core::write;

use embassy_net::{Config, Ipv4Address, Ipv4Cidr, StaticConfigV4, Stack, StackResources, tcp::TcpSocket};
use static_cell::StaticCell;
use heapless::Vec;
use embedded_io_async::Write as _;

use embassy_lab_utils::{init_wifi, init_network_stack};
use core::sync::atomic::{AtomicBool, Ordering};

static DISPENSE_REQUESTED: AtomicBool = AtomicBool::new(false);
static STACK_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
static RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();
static TX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = init(Default::default());

    let (wifi_device, mut controller) = init_wifi!(&spawner, p).await;
    controller.start_ap_open("Feeder-AP", 6).await;
    info!("✅ Access Point 'Feeder-AP' activat");

    let config = Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 4, 1), 24),
        gateway: Some(Ipv4Address::new(192, 168, 4, 1)),
        dns_servers: Vec::new(),
    });

    let mut stack = init_network_stack::<4>(&spawner, wifi_device, &STACK_RESOURCES, config);
    let rx = RX_BUF.init([0; 1024]);
    let tx = TX_BUF.init([0; 1024]);

    spawner.spawn(web_server(stack, rx, tx)).unwrap();

    let mut config = PwmConfig::default();
    config.top = 0xB71A;
    config.divider = 64_i32.to_fixed();
    let duty_closed = (1350 * config.top as usize) / 20_000;
    let duty_open = (2000 * config.top as usize) / 20_000;
    let mut servo = Pwm::new_output_a(p.PWM_SLICE1, p.PIN_18, config);
    servo.set_duty_cycle(duty_closed as u16).unwrap();

    let button = Input::new(p.PIN_10, Pull::Up);
    let mut trig = Output::new(p.PIN_2, Level::Low);
    let echo = Input::new(p.PIN_3, Pull::None);

    let i2c = I2c::new_blocking(p.I2C0, p.PIN_1, p.PIN_0, i2c::Config::default());
    let mut delay = Delay;
    let mut lcd = HD44780::new_i2c(i2c, 0x27, &mut delay).unwrap();
    lcd.reset(&mut delay).unwrap();
    lcd.clear(&mut delay).unwrap();
    lcd.set_display(Display::On, &mut delay).unwrap();
    lcd.write_str("Pet Feeder Ready", &mut delay).unwrap();

    loop {
        let pressed = button.is_low();
        let dist = measure_distance(&mut trig, &echo).await;

        
        if DISPENSE_REQUESTED.swap(false, Ordering::Relaxed) {
            info!("🧠 Dispense signal received from web!");
            lcd.clear(&mut delay).unwrap();
            lcd.write_str("Feeding (web)...", &mut delay).unwrap();
            servo.set_duty_cycle(duty_open as u16).unwrap();
            Timer::after_secs(2).await;
            servo.set_duty_cycle(duty_closed as u16).unwrap();
            lcd.set_cursor_pos(0x40, &mut delay).unwrap();
            lcd.write_str("Done!", &mut delay).unwrap();
        }

        lcd.clear(&mut delay).unwrap();

        if dist > 0 && dist <= 20 {
            lcd.write_str("Object detected", &mut delay).unwrap();
            lcd.set_cursor_pos(0x40, &mut delay).unwrap();
            let mut line: String<32> = String::new();
            let _ = write!(line, "Distance: {} cm", dist);
            let _ = lcd.write_str(&line, &mut delay);
        } else if pressed {
            lcd.write_str("Manual trigger", &mut delay).unwrap();
            lcd.set_cursor_pos(0x40, &mut delay).unwrap();
            lcd.write_str("Button pressed...", &mut delay).unwrap();
        } else {
            lcd.write_str("Pet Feeder Ready", &mut delay).unwrap();
            lcd.set_cursor_pos(0x40, &mut delay).unwrap();
            lcd.write_str("Waiting for pet...", &mut delay).unwrap();
        }

        if pressed || (dist > 0 && dist <= 20) {
            servo.set_duty_cycle(duty_open as u16).unwrap();
            Timer::after_secs(2).await;
            servo.set_duty_cycle(duty_closed as u16).unwrap();

            if pressed {
                Timer::after_secs(5).await;
                while button.is_low() {
                    Timer::after_millis(50).await;
                }
            } else {
                Timer::after_secs(10).await;
            }
        }

        Timer::after_millis(100).await;
    }
}
#[embassy_executor::task]
async fn web_server(stack: Stack<'static>, rx: &'static mut [u8; 1024], tx: &'static mut [u8; 1024]) {
    loop {
        let mut socket = TcpSocket::new(stack, rx, tx);

        if socket.accept(80).await.is_err() {
            continue;
        }

        let mut buf = [0u8; 512];
        let n = match socket.read(&mut buf).await {
            Ok(n) => n,
            Err(_) => continue,
        };

        let request = core::str::from_utf8(&buf[..n]).unwrap_or_default();
        info!("📥 Cerere primită: {}", request);

        if request.contains("GET /dispense") {
            DISPENSE_REQUESTED.store(true, Ordering::Relaxed);
            let _ = socket.write_all(RESPONSE_DISPENSE.as_bytes()).await;
        } else {
            use core::fmt::Write;
            let mut header: heapless::String<512> = heapless::String::new();
            let _ = write!(
                header,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                HTML_BODY.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(HTML_BODY.as_bytes()).await;
        }

        let _ = socket.flush().await;
        Timer::after(Duration::from_millis(200)).await;
    }
}

const RESPONSE_DISPENSE: &str = concat!(
    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n",
    "🍖 Hrănire în curs... Servo activat timp de 2 secunde."
);

const HTML_BODY: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
  <title>Pet Feeder Timer</title>
  <style>
    body {
      font-family: sans-serif;
      display: flex;
      justify-content: center;
      align-items: center;
      min-height: 100vh;
      background-color: #fce4ec;
      margin: 0;
      color: #4a4a4a;
    }
    .container {
      background-color: #fff;
      padding: 30px;
      border-radius: 10px;
      box-shadow: 0 0 15px rgba(0, 0, 0, 0.1);
      text-align: center;
    }
    h1 {
      color: #e91e63;
      margin-bottom: 20px;
    }
    .timer-container {
      margin-bottom: 20px;
    }
    .timer-container label {
      margin-right: 10px;
      font-size: 1.1em;
    }
    .timer-container input[type='number'] {
      padding: 8px;
      border: 1px solid #f8bbd0;
      border-radius: 5px;
      width: 60px;
      text-align: center;
      font-size: 1em;
    }
    #timer-display {
      font-size: 4em;
      margin-bottom: 25px;
      color: #9c27b0;
      font-weight: bold;
    }
    button {
      background-color: #ff80ab;
      color: white;
      border: none;
      padding: 12px 25px;
      font-size: 1.1em;
      border-radius: 5px;
      cursor: pointer;
      transition: background-color 0.3s ease;
      margin-bottom: 20px;
    }
    button:hover {
      background-color: #f50057;
    }
    button:disabled {
      background-color: #e1bee7;
      cursor: not-allowed;
    }
    .status {
      font-size: 1.1em;
      padding: 10px;
      border-radius: 5px;
    }
    .status-waiting {
      background-color: #f3e5f5;
      color: #8e24aa;
    }
    .status-running {
      background-color: #f8bbd0;
      color: #c2185b;
    }
    .status-finished {
      background-color: #ba68c8;
      color: white;
    }
  </style>
</head>
<body>
  <div class="container">
    <h1>Pet Feeder</h1>
    <div class="timer-container">
      <label for="seconds">Set Timer (seconds):</label>
      <input type="number" id="seconds" value="60" min="1" max="60">
    </div>
    <div id="timer-display">01:00</div>
    <button id="startButton">Start Countdown</button>
    <br />
    <button id="dispenseNowButton">Dispense Now</button>
    <div id="status" class="status-waiting">Waiting for timer to finish...</div>
  </div>
  <script>
    const secondsInput = document.getElementById('seconds');
    const timerDisplay = document.getElementById('timer-display');
    const startButton = document.getElementById('startButton');
    const statusDiv = document.getElementById('status');
    let countdown, timeLeft;

    function formatTime(s) {
      const m = Math.floor(s / 60);
      const sec = s % 60;
      return `${m.toString().padStart(2,'0')}:${sec.toString().padStart(2,'0')}`;
    }

    function updateTimerDisplay() {
      timerDisplay.textContent = formatTime(timeLeft);
    }

    function startTimer() {
      if (countdown) clearInterval(countdown);
      timeLeft = parseInt(secondsInput.value);
      if (isNaN(timeLeft) || timeLeft < 1 || timeLeft > 60) {
        alert("Please enter a valid number of seconds (1–60).");
        return;
      }
      updateTimerDisplay();
      statusDiv.textContent = "Timer running...";
      statusDiv.className = "status-running";
      startButton.disabled = true;
      secondsInput.disabled = true;

      countdown = setInterval(() => {
        timeLeft--;
        updateTimerDisplay();
        if (timeLeft < 0) {
          clearInterval(countdown);
          timerDisplay.textContent = "00:00";
          statusDiv.textContent = "Food dispensed!";
          statusDiv.className = "status-finished";
          alert("Time's up! Food should be dispensed.");
          fetch("/dispense");
          startButton.disabled = false;
          secondsInput.disabled = false;
        }
      }, 1000);
    }

    startButton.addEventListener("click", startTimer);
    document.getElementById("dispenseNowButton").addEventListener("click", () => {
      fetch("/dispense");
      alert("Dispense triggered immediately.");
    });

    secondsInput.addEventListener("input", () => {
      const secs = parseInt(secondsInput.value);
      if (!isNaN(secs) && secs >= 1 && secs <= 60) {
        timerDisplay.textContent = formatTime(secs);
      }
    });

    timerDisplay.textContent = formatTime(parseInt(secondsInput.value));
  </script>
</body>
</html>
"#;

async fn measure_distance(trig: &mut Output<'_>, echo: &Input<'_>) -> u32 {
    trig.set_low();
    Timer::after_micros(2).await;
    trig.set_high();
    Timer::after_micros(10).await;
    trig.set_low();

    let mut timeout = 30_000;
    while echo.is_low() {
        Timer::after_micros(1).await;
        if timeout == 0 {
            return 0;
        }
        timeout -= 1;
    }

    let start = Instant::now();

    timeout = 30_000;
    while echo.is_high() {
        Timer::after_micros(1).await;
        if timeout == 0 {
            return 0;
        }
        timeout -= 1;
    }

    let pulse_us = start.elapsed().as_micros() as u32;
    pulse_us / 58
}
