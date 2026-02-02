# Microprocessor architecture (MA/PM) Lab

![PMRust Lab logo](https://gitlab.cs.pub.ro/pmrust/pmrust.pages.upb.ro/-/raw/main/website/static/img/logo.svg?ref_type=heads)

This project is an embedded system for automatic pet feeding, built on the RP2350 platform Raspberry Pi Pico 2W and using the Embassy ecosystem for async programming, WiFi connectivity, and a web interface.

## Project structure

- **src/main.rs** – Main application code, contains logic for servo control, ultrasonic sensor, button, LCD, and web server.
- **Cargo.toml** – Project dependencies and configuration.
- **embassy-lab-utils/** – Utilities for WiFi and network stack initialization.
- **cyw43-firmware/** – Firmware for the CYW43 WiFi chip.


## Library Usage

•	embassy-executor:
	- Asynchronous runtime for embedded Rust
	- Runs main() and web_server() as async tasks
•	embassy-rp:
	- HAL for Raspberry Pi Pico 2W
	- Configures PWM (servo), GPIO (button, ultrasonic), I²C (LCD)
•	embassy-time:
	- Async timing utilities (timers, delays)
	- Used for Timer::after_secs(), Timer::after_micros() for delays
•	hd44780-driver:
	- Driver for HD44780 LCD over I²C via PCF8574 expander
	- Initializes and writes messages to the 1602 LCD
•	heapless:
	- Stack-allocated types like String<32> without heap allocation
	- Builds display messages for the LCD and HTTP headers
•	fixed:
	- Fixed-point arithmetic types
	- Sets PWM frequency divider via to_fixed()
•	defmt:
	- Efficient logging framework for embedded devices
	- Displays debug/info messages via info!() during runtime
•	defmt-rtt:
	- Sends defmt logs over RTT (Real-Time Transfer) via USB
	- Streams logs to the host for debugging
•	panic-probe:
	- Panic handler that outputs the panic reason through defmt
	- Captures and logs panics during runtime
•	core::fmt::Write:
	- Enables formatted string output to heapless String
	- Used with write!() macro to write to a String<32>
•	embassy-net:
	- Async TCP/IP networking for embedded systems using Embassy
	- Sets up a TCP web server and serves HTTP responses
•	embedded-io-async:
	- Async traits for I/O operations (read, write, flush)
	- Used for writing HTTP responses with write_all()
•	core::sync::atomic:
	-Low-level atomic types for safe concurrency
	- AtomicBool signals a dispense request from the web handler
•	static_cell:
	- Safe static allocation for objects that live forever
	- Used for persistent socket buffers
•	core::str:
	- Core string operations (UTF-8 parsing)
	- Interprets incoming TCP data as a string
•	embassy_lab_utils:
	- Custom utilities for Wi-Fi and network initialization 
	- Provides init_wifi!() and init_network_stack() macros
•	core::write:
	- Macro support to use write!() in no_std
	- Formats LCD text and HTTP headers


## Main Structures and Functions

### Global Structures

- `DISPENSE_REQUESTED: AtomicBool` – Signals if feeding was requested from the web.
- `STACK_RESOURCES, RX_BUF, TX_BUF: StaticCell` – Buffers and resources for the network stack.

### Main Function: `main`

- Initializes peripherals: PWM for servo, GPIO for button and ultrasonic sensor, I2C for LCD.
- Configures and starts the WiFi Access Point.
- Initializes the network stack and spawns the web server task.
- Runs the main loop:
  - Measures distance with the ultrasonic sensor (`measure_distance`).
  - Updates the LCD with the current state (waiting, object detected, manual/automatic feeding).
  - Controls the servo to dispense food on request (button, web, or animal detection).

### Web Server: `web_server`

- Listens for TCP connections on port 80.
- Responds to HTTP requests:
  - `/dispense` – sets the feeding flag and replies with a text message.
  - Any other request – sends the HTML page with timer and feeding button.

### Distance Measurement: `measure_distance`

- Sends an ultrasonic pulse and measures the time until the echo returns.
- Calculates the distance in centimeters using the formula: `pulse_us / 58`.


## Web Interface

The web page provides:
- Timer setting for automatic feeding.
- Button for immediate feeding.
- Visual feedback on the system status.


## Peripheral Usage Examples

- **PWM for servo**: `Pwm::new_output_a(...)`, `set_duty_cycle(...)`
- **GPIO for button and sensor**: `Input::new(...)`, `Output::new(...)`, `is_low()`, `set_high()`
- **I2C for LCD**: `HD44780::new_i2c(...)`, `write_str(...)`, `clear(...)`
- **Timer/Delay**: `Timer::after_secs(...)`, `Timer::after_micros(...)`
