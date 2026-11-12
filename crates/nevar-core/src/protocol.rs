//! Datagram wire format. The first byte of every datagram says what it carries.

/// Probe packet to be echoed back: `[type][seq u32][send time u64][padding]`.
pub const PROBE_REQUEST: u8 = 0;
/// An echoed probe packet, otherwise identical to the request.
pub const PROBE_REPLY: u8 = 1;
/// One 20 ms Opus voice frame: `[type][seq u32][flags u8][opus bytes]`.
pub const VOICE: u8 = 2;

/// One 20 ms Opus stereo frame of shared music, laid out like [`VOICE`].
pub const MUSIC: u8 = 3;

/// Header length of [`VOICE`] and [`MUSIC`] datagrams.
pub const VOICE_HEADER: usize = 6;

/// Last frame before the sender went quiet on purpose (voice activation, push-to-talk
/// release, mute; for music, sharing stopped). Lets the receiver tell deliberate silence from a late network.
pub const FLAG_END_OF_SPEECH: u8 = 1;
