# PktMon
![Crates.io Version](https://img.shields.io/crates/v/pktmon)

PktMon is a library for capturing network packets on Windows using the
PktMon service, which is included by default with Windows 10 and later.

See [here](https://learn.microsoft.com/en-us/windows-server/networking/technologies/pktmon/pktmon)
for more information about the PktMon service.

See the [Documentation](https://docs.rs/pktmon/latest/x86_64-pc-windows-msvc/pktmon/)
for more information about the library.

## Features

- Easy-to-use high-level interface for packet capture
- Filter support for protocol, ports, IP addresses, and more

## Requirements

- Windows 10 or later
- Administrator privileges are required to talk to the PktMon service

## Installation

```bash
cargo add pktmon
```

## Usage

```rust
use pktmon::{Capture, filter::{PktMonFilter, TransportProtocol}};

fn main() {
    // Create a new capture instance
    let mut capture = Capture::new().unwrap();

    // Add a filter to capture UDP traffic on port 1234
    capture.add_filter(PktMonFilter {
        name: "UDP Filter".to_string(),
        transport_protocol: Some(TransportProtocol::UDP),
        port: 1234.into(),

        ..PktMonFilter::default()
    }).unwrap();
    
    // Start capturing
    capture.start().unwrap();
    
    // Get and print the next packet
    let packet = capture.next_packet().unwrap();
    println!("{:?}", packet.payload);
    
    // Stop capturing
    capture.stop().unwrap();
    
    // Unload the driver when done
    capture.unload().unwrap();
}
```

## Truncation size

`CaptureOptions` sets how the real-time stream (Windows 11+) is opened. The legacy backend ignores it.

```rust
use pktmon::{Capture, CaptureOptions};

let capture = Capture::new_with_options(CaptureOptions {
    truncation_size: 65535, // bytes, at least 64, default 9000
    ..CaptureOptions::default()
}).unwrap();
println!("frames are cut at {} bytes", capture.granted_truncation_size());
```

Microsoft documents 9000 as the maximum truncation size, and the driver's behavior above it is unverified. A larger size is asked for as given. If the real-time stream cannot be created with it, the capture retries once with 9000 before it falls back to the legacy backend, so asking for more never turns the real-time stream into the legacy one by itself. `granted_truncation_size()` returns the size in effect, or 0 on the legacy backend. `Capture::new()` and `Capture::new_with_buffer_size_multiplier()` behave as before.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
