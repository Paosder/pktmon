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

## Stream events and drop reasons

The real-time backend counts what the driver reports, so an app can read numbers instead of parsing logs. Both functions return process-wide totals since the process started, so keep the value from when a capture began and subtract it. They hold counts and driver codes only, never packet bytes or addresses. The legacy backend reports neither.

```rust
let events = pktmon::stream_events();
println!(
    "{} warnings, {} errors, {} insufficient buffer (largest packet {} bytes)",
    events.warnings, events.errors, events.insufficient_buffer, events.insufficient_buffer_max_len
);

let drops = pktmon::drop_events();
for (reason, count) in drops.reasons.iter().filter(|(reason, _)| *reason != 0) {
    println!("drop reason {reason}: {count}");
}
```

`StreamEvents` covers the stream-started, stopped and fatal events, the warning and error process-info events, the ones with reason `REASON_INSUFFICIENT_BUFFER` (122) by packet length class, and the buffer and truncation sizes the driver reported at the last start. `DropEvents` counts the packets whose metadata carries a drop reason. The first `DROP_REASON_SLOTS` (8) distinct reasons each get a slot, and the rest are summed in `other`.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
