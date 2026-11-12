# nevar

Arkadaşlar arasında sesli sohbet. Sunucu yok, hesap yok: bilgisayarlar birbirine doğrudan, uçtan uca şifreli bağlanır.
Sadece windowsta çalışır.

## Kurulum

[Releases](../../releases/latest) sayfasından `nevar_..._x64-setup.exe` dosyasını indirip çalıştır. Windows "bilinmeyen yayıncı" uyarısı verirse **Ek bilgi → Yine de çalıştır**. Yeni sürümler uygulama içinden otomatik gelir.

## How it works

- **Networking:** [iroh](https://iroh.computer). Peers are dialed by public key; NAT hole punching gives a direct path, with iroh's public relays as fallback. QUIC/TLS end to end.
- **Voice:** Opus (libopus) at 32 kbps in 20 ms frames over QUIC datagrams, an adaptive jitter buffer with FEC recovery and loss concealment.
- **Audio processing:** WebRTC AEC3 echo cancellation and AGC2 (via [sonora](https://crates.io/crates/sonora)), RNNoise noise suppression and voice activity detection (via [nnnoiseless](https://crates.io/crates/nnnoiseless)).
- **App:** Tauri 2 + React/TypeScript; all networking and audio live in Rust.

## Layout

- `crates/nevar-core`: networking, voice pipeline, friends/calls service
- `crates/nevar-cli`: command-line test tool (network probe, calls)
- `app/`: the desktop app (Tauri)

## Development

Needs Rust, Node + pnpm and the Visual Studio C++ Build Tools (with CMake; see `.cargo/config.toml`).

```
cd app
pnpm install
pnpm tauri dev
```

Tests (some need internet, they start real nodes): `cargo test -p nevar-core`

### Test bot

A pretend friend for trying the app alone. It runs the app's real service with a virtual sound card: it accepts friend requests, answers calls and says your voice back 1.5 s later, replies to messages, and can call you, play a tune, or fake a bad network.

```
cargo run --release -p nevar-cli -- bot
```

Paste the code it prints into "Arkadaş ekle", then type `yardım` in its terminal for the commands. For a group call, start a second bot with `--profile 2` and add it to a call with the first.
