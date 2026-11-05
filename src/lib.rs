//! # PktMon
//!
//! PktMon is a library for capturing network packets on Windows using the
//! PktMon driver, which is included by default with Windows 10 and later.
//!
//! See [here](https://learn.microsoft.com/en-us/windows-server/networking/technologies/pktmon/pktmon)
//! for more information about the PktMon service.
//!
//! ## Features
//!
//! - Easy-to-use high-level interface for packet capture
//! - Filter support for protocol, ports, IP addresses, and more
//! - Support for reading packets from ETL files
//!
//! ## Requirements
//!
//! - Windows 10 or later
//! - Administrator privileges are required to talk to the PktMon service
//!
//! ## Usage
//!
//! See [Capture] for more information on live capture and [EtlCapture] for working with ETL files.
//!
//! ```no_run
//! use pktmon::{Capture, filter::{PktMonFilter, TransportProtocol}};
//!
//! fn main() {
//!     // Create a new capture instance
//!     let mut capture = Capture::new().unwrap();
//!
//!     // Add a filter to capture UDP traffic on port 1234
//!     capture.add_filter(PktMonFilter {
//!         name: "UDP Filter".to_string(),
//!         transport_protocol: Some(TransportProtocol::UDP),
//!         port: 1234.into(),
//!
//!         ..PktMonFilter::default()
//!     }).unwrap();
//!     
//!     // Start capturing
//!     capture.start().unwrap();
//!     
//!     // Get and print the next packet
//!     let packet = capture.next_packet().unwrap();
//!     println!("{:?}", packet.payload);
//!     
//!     // Stop capturing
//!     capture.stop().unwrap();
//!     
//!     // Unload the driver when done
//!     capture.unload().unwrap();
//! }
//! ```

use filter::PktMonFilter;
use legacy::{EtlConsumer, LegacyBackend};
use log::{debug, info};
use realtime::RealTimeBackend;
pub use delay::{DELAY_BOUNDS_MS, DELAY_BUCKETS, DelaySnapshot, take_delivery_delay};
pub use events::{
    DROP_REASON_SLOTS, DropEvents, REASON_INSUFFICIENT_BUFFER, StreamEvents, drop_events,
    stream_events,
};
pub use missed::{MissedPackets, missed_packets};
pub use realtime::packets_dropped_after_receiver_closed;
use std::{
    fmt::Debug,
    io,
    path::Path,
    sync::mpsc::{RecvError, RecvTimeoutError, TryRecvError},
    time::Duration,
};

mod clock;
mod ctypes;
mod delay;
mod dll;
mod events;
pub mod filter;
mod legacy;
mod limits;
mod lock;
mod missed;
mod realtime;
pub mod stream;
mod util;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PacketPayload {
    Unknown(Vec<u8>),
    Ethernet(Vec<u8>),
    WiFi(Vec<u8>),
    IP(Vec<u8>),
    HTTP(Vec<u8>),
    TCP(Vec<u8>),
    UDP(Vec<u8>),
    ARP(Vec<u8>),
    ICMP(Vec<u8>),
    ESP(Vec<u8>),
    AH(Vec<u8>),
    L4Payload(Vec<u8>),
}

impl PacketPayload {
    pub fn to_vec(&self) -> &Vec<u8> {
        match self {
            PacketPayload::Unknown(payload) => payload,
            PacketPayload::Ethernet(payload) => payload,
            PacketPayload::WiFi(payload) => payload,
            PacketPayload::IP(payload) => payload,
            PacketPayload::HTTP(payload) => payload,
            PacketPayload::TCP(payload) => payload,
            PacketPayload::UDP(payload) => payload,
            PacketPayload::ARP(payload) => payload,
            PacketPayload::ICMP(payload) => payload,
            PacketPayload::ESP(payload) => payload,
            PacketPayload::AH(payload) => payload,
            PacketPayload::L4Payload(payload) => payload,
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Packet {
    /// Component ID where the packet was captured
    /// Use this to group packets into individual streams
    pub component_id: u16,
    /// Packet payload differentiated by captured protocol
    pub payload: PacketPayload,
}

pub(crate) trait CaptureBackend: Debug + Send {
    fn start(&mut self) -> io::Result<()>;
    fn stop(&mut self) -> io::Result<()>;
    fn unload(&mut self) -> io::Result<()>;
    fn add_filter(&mut self, filter: PktMonFilter) -> io::Result<()>;
    fn next_packet(&self) -> Result<Packet, RecvError>;
    fn next_packet_timeout(&self, timeout: Duration) -> Result<Packet, RecvTimeoutError>;
    fn try_next_packet(&self) -> Result<Packet, TryRecvError>;

    #[cfg(feature = "tokio")]
    fn notify(&self) -> Option<std::sync::Arc<tokio::sync::Notify>>;
}

#[derive(Debug)]
enum DispatchCaptureBackend {
    Legacy(LegacyBackend),
    RealTime(RealTimeBackend),
}

macro_rules! dispatch {
    ($backend:ident, $method:ident($($args:tt)*)) => {
        match $backend {
            DispatchCaptureBackend::Legacy(backend) => backend.$method($($args)*),
            DispatchCaptureBackend::RealTime(backend) => backend.$method($($args)*),
        }
    };
}

impl CaptureBackend for DispatchCaptureBackend {
    fn start(&mut self) -> io::Result<()> {
        dispatch!(self, start())
    }

    fn stop(&mut self) -> io::Result<()> {
        dispatch!(self, stop())
    }

    fn unload(&mut self) -> io::Result<()> {
        dispatch!(self, unload())
    }

    fn add_filter(&mut self, filter: PktMonFilter) -> io::Result<()> {
        dispatch!(self, add_filter(filter))
    }

    fn next_packet(&self) -> Result<Packet, RecvError> {
        dispatch!(self, next_packet())
    }

    fn next_packet_timeout(&self, timeout: Duration) -> Result<Packet, RecvTimeoutError> {
        dispatch!(self, next_packet_timeout(timeout))
    }

    fn try_next_packet(&self) -> Result<Packet, TryRecvError> {
        dispatch!(self, try_next_packet())
    }

    #[cfg(feature = "tokio")]
    fn notify(&self) -> Option<std::sync::Arc<tokio::sync::Notify>> {
        dispatch!(self, notify())
    }
}

/// A packet capture instance that uses the Windows PktMon driver to capture network traffic.
///
/// The `Capture` struct provides a high-level interface for capturing network packets using
/// the Windows PktMon driver. It manages the driver lifecycle, ETW session, and packet
/// consumption.
///
/// # Examples
///
/// ```no_run
/// use pktmon::{Capture, filter::{PktMonFilter, TransportProtocol}};
///
/// // Create a new capture instance
/// let mut capture = Capture::new().unwrap();
///
/// // Add a filter to capture UDP traffic on port 23301
/// capture.add_filter(PktMonFilter {
///     name: "UDP Filter".to_string(),
///     transport_protocol: Some(TransportProtocol::UDP),
///     port: 23301.into(),
///     ..PktMonFilter::default()
/// }).unwrap();
///
/// // Start capturing
/// capture.start().unwrap();
///
/// // Get the next packet
/// let packet = capture.next_packet().unwrap();
/// println!("{:?}", packet.payload);
///
/// // Stop capturing
/// capture.stop().unwrap();
///
/// // Unload the driver when done
/// capture.unload().unwrap();
/// ```
///
/// # Driver Lifecycle
///
/// The PktMon driver is loaded when creating a new `Capture` instance and remains loaded
/// until either:
/// - The `unload()` method is explicitly called
/// - The `Capture` instance is dropped
///
/// # Filters
///
/// Filters should be added before starting the capture using [`add_filter()`](Capture::add_filter).
/// Multiple filters can be added to capture different types of traffic.
/// If a packet matches any filter, it will be captured.
///
/// # Resource Cleanup
///
/// The `Capture` struct implements `Drop` to ensure proper cleanup of resources. When dropped:
/// 1. The capture is stopped if running
/// 2. The ETW session is deactivated
/// 3. The ETW consumer is stopped
///
/// However, it's recommended to explicitly call `stop()` and `unload()` when done to handle
/// any potential errors.
///
/// # Thread Safety
///
/// The `Capture` struct can be safely shared between threads using standard synchronization
/// primitives like `Arc<Mutex<Capture>>`. See the examples directory for a threaded example.
pub struct Capture {
    backend: DispatchCaptureBackend,
    granted_truncation_size: u16,
    running: bool,
}

impl Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PktMon Capture {{ backend: {:?} }}", self.backend)
    }
}

/// The real-time stream buffer size multiplier used when the caller does not pick one.
///
/// The driver sizes the stream buffer as this multiple of its 256 KiB base buffer, and accepts a
/// multiplier from 1 to 10 (Microsoft's `PacketMonitorCreateRealtimeStream` documentation). The
/// largest is 2.5 MiB. One base buffer holds about 29 packets at the 9000-byte truncation, and the
/// stream reports a packet once for each component it passes, so the default is the largest the
/// driver accepts. The 640 MiB private region once blamed on this setting came from the legacy
/// ETW session's 16 MiB buffers, not from the real-time stream.
pub const DEFAULT_BUFFER_SIZE_MULTIPLIER: u16 = limits::MAX_BUFFER_SIZE_MULTIPLIER;

/// How a [`Capture`] opens its real-time stream.
///
/// The legacy backend ignores all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOptions {
    /// How many 256 KiB base buffers the stream reserves, brought into the 1 to 10 the driver
    /// accepts. See [DEFAULT_BUFFER_SIZE_MULTIPLIER].
    pub buffer_size_multiplier: u16,
    /// The size in bytes at which the driver cuts a frame, at least 64. The default is 9000.
    ///
    /// Microsoft documents 9000 as the maximum, and the driver's behavior above it is unverified.
    /// A larger size is asked for as given, so that a receive-coalesced frame of up to 64 KB can
    /// arrive whole. If the real-time stream cannot be created with it, the capture retries the
    /// real-time stream once with 9000 before it falls back to the legacy backend. Read the size
    /// in effect from [`Capture::granted_truncation_size()`].
    pub truncation_size: u16,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            buffer_size_multiplier: DEFAULT_BUFFER_SIZE_MULTIPLIER,
            truncation_size: limits::DOCUMENTED_MAX_TRUNCATION_SIZE,
        }
    }
}

impl Capture {
    /// Create a new capture instance with the default [`CaptureOptions`].
    pub fn new() -> io::Result<Self> {
        Self::new_with_options(CaptureOptions::default())
    }

    /// Create a new capture instance whose real-time stream buffer is `buffer_size_multiplier`
    /// times the driver's 256 KiB base buffer. A value outside 1 to 10, the range the driver
    /// accepts, is brought into it.
    ///
    /// The multiplier only reaches the real-time backend (Windows 11+). A capture that falls back
    /// to the legacy backend ignores it. See [DEFAULT_BUFFER_SIZE_MULTIPLIER] for the default.
    pub fn new_with_buffer_size_multiplier(buffer_size_multiplier: u16) -> io::Result<Self> {
        Self::new_with_options(CaptureOptions {
            buffer_size_multiplier,
            ..CaptureOptions::default()
        })
    }

    /// Create a new capture instance that opens its real-time stream as `options` says.
    ///
    /// The options only reach the real-time backend (Windows 11+). A capture that falls back to
    /// the legacy backend ignores them. See [`CaptureOptions::truncation_size`] for what happens
    /// when the driver refuses a size above 9000.
    pub fn new_with_options(options: CaptureOptions) -> io::Result<Self> {
        // Try to use the RealTimeBackend first (Windows 11+)
        // Fall back to LegacyBackend if RealTimeBackend is not available
        let (backend, granted_truncation_size) =
            match limits::open_with_truncation_fallback(options.truncation_size, |size| {
                RealTimeBackend::new(options.buffer_size_multiplier, size)
            }) {
                Ok((backend, granted)) => (DispatchCaptureBackend::RealTime(backend), granted),
                Err(_) => (DispatchCaptureBackend::Legacy(LegacyBackend::new()?), 0),
            };

        Ok(Self {
            backend,
            granted_truncation_size,
            running: false,
        })
    }

    /// The truncation size the real-time stream was created with, in bytes.
    ///
    /// This is the requested size brought up to the minimum of 64, or 9000 when the driver
    /// refused a larger request. It is 0 when the capture uses the legacy backend.
    pub fn granted_truncation_size(&self) -> u16 {
        self.granted_truncation_size
    }

    /// Start the capture.
    ///
    /// Ensure to add filters before starting the capture.
    pub fn start(&mut self) -> io::Result<()> {
        if self.running {
            return Ok(());
        }

        debug!("Starting capture...");

        self.backend.start()?;
        self.running = true;

        info!("Capture started");

        Ok(())
    }

    /// Stop the capture.
    ///
    /// You may still receive packets after stopping the capture.
    pub fn stop(&mut self) -> io::Result<()> {
        if !self.running {
            return Ok(());
        }

        debug!("Stopping capture...");

        self.running = false;
        self.backend.stop()?;

        info!("Capture stopped");

        Ok(())
    }

    /// Unload the PktMon driver.
    ///
    /// This will ensure the driver isn't used after this.
    pub fn unload(mut self) -> io::Result<()> {
        if self.running {
            self.stop()?;
        }

        self.backend.unload()?;

        // Take self and drop it to ensure the driver isn't used after this
        drop(self);

        Ok(())
    }

    /// Add a filter to the capture.
    pub fn add_filter(&mut self, filter: PktMonFilter) -> io::Result<()> {
        self.backend.add_filter(filter)?;
        Ok(())
    }

    /// Get the next packet from the capture.
    ///
    /// Returns an error if the capture isn't running.
    pub fn next_packet(&self) -> Result<Packet, RecvError> {
        self.backend.next_packet()
    }

    /// Try to get the next packet from the capture.
    ///
    /// Returns an error if the capture isn't running.
    pub fn try_next_packet(&self) -> Result<Packet, TryRecvError> {
        self.backend.try_next_packet()
    }

    /// Get the next packet from the capture with a timeout.
    ///
    /// Returns an error if the capture isn't running or if the timeout is reached.
    pub fn next_packet_timeout(&self, timeout: Duration) -> Result<Packet, RecvTimeoutError> {
        self.backend.next_packet_timeout(timeout)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if self.running {
            if let Err(e) = self.stop() {
                debug!("Failed to stop capture: {:?}", e);
            }
        }
    }
}

/// A packet capture reader that processes packets from an ETL file.
///
/// The `EtlCapture` struct provides a high-level interface for reading network packets
/// from an Event Trace Log (ETL) file. This is useful for offline analysis of packets
/// captured using the PktMon driver or other ETW-based packet capture tools.
///
/// # Examples
///
/// ```no_run
/// use pktmon::EtlCapture;
/// use std::path::Path;
///
/// // Create a new ETL capture reader
/// let mut etl_capture = EtlCapture::new("C:\\path\\to\\capture.etl").unwrap();
///
/// // Get all packets from the file
/// let packets = etl_capture.packets().unwrap();
/// println!("Read {} packets from ETL file", packets.len());
///
/// // Process each packet
/// for packet in packets {
///     println!("Packet payload size: {}", packet.payload.len());
/// }
/// ```
///
/// # Resource Cleanup
///
/// The `EtlCapture` struct implements `Drop` to ensure proper cleanup of resources.
/// However, it's recommended to explicitly stop processing when done to handle any
/// potential errors.
pub struct EtlCapture {
    consumer: EtlConsumer,
}

impl Debug for EtlCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PktMon ETL Capture")
    }
}

impl EtlCapture {
    /// Create a new ETL capture reader for the specified ETL file.
    ///
    /// # Arguments
    ///
    /// * `etl_path` - Path to the ETL file to process
    ///
    /// # Returns
    ///
    /// A result containing the `EtlCapture` or an error.
    pub fn new<P: AsRef<Path>>(etl_path: P) -> io::Result<Self> {
        let consumer = match EtlConsumer::new(etl_path) {
            Ok(consumer) => consumer,
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        };

        Ok(Self { consumer })
    }

    /// Start processing the ETL file.
    ///
    /// This function starts processing the ETL file in a background thread.
    /// Packets can be retrieved using the `next_packet()` method.
    ///
    /// # Returns
    ///
    /// A result indicating success or an error.
    pub fn start(&mut self) -> io::Result<()> {
        match self.consumer.process() {
            Ok(()) => Ok(()),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    /// Stop processing the ETL file.
    ///
    /// This function stops any ongoing processing and releases resources.
    ///
    /// # Returns
    ///
    /// A result indicating success or an error.
    pub fn stop(&mut self) -> io::Result<()> {
        match self.consumer.stop() {
            Ok(()) => Ok(()),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    /// Get the next packet from the ETL file.
    ///
    /// Returns an error if processing hasn't been started or if there are no more packets.
    ///
    /// # Returns
    ///
    /// A result containing the next packet or an error.
    pub fn next_packet(&self) -> Result<Packet, RecvError> {
        self.consumer.receiver.recv()
    }

    /// Process the ETL file synchronously and return all packets.
    ///
    /// This function blocks until the entire file has been processed or an error occurs.
    /// All packets are collected and returned in a vector.
    ///
    /// # Returns
    ///
    /// A result containing a vector of packets or an error.
    pub fn packets(&mut self) -> io::Result<Vec<Packet>> {
        match self.consumer.process_sync() {
            Ok(packets) => Ok(packets),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }
}

impl Drop for EtlCapture {
    fn drop(&mut self) {
        if let Err(e) = self.consumer.stop() {
            debug!("Failed to stop ETL capture: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_that_does_not_pick_a_multiplier_uses_the_largest_the_driver_accepts() {
        assert_eq!(DEFAULT_BUFFER_SIZE_MULTIPLIER, 10);
    }

    #[test]
    fn default_options_reproduce_the_settings_the_crate_always_used() {
        assert_eq!(
            CaptureOptions::default(),
            CaptureOptions {
                buffer_size_multiplier: 10,
                truncation_size: 9000,
            }
        );
    }
}
