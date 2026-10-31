<h1 align="center">nevar</h1>
<p align="center">Arkadaşlar arasında sesli sohbet. Sunucu yok, hesap yok.</p>

---

*FUCK* `discord, teamspeak, stoat`<br>
stoat berbat çalışıyor, delay var. discord TR'de yasaklı. teamspeak kötü UI ve paralı.

**nevar** en basiti: neredeyse hiç RAM yemez, anlıktır, hafiftir, gerçek zamanlı konuşma gibi hissettirir. Bilgisayarlar birbirine doğrudan ve uçtan uca şifreli bağlanır.

> [!WARNING]
> Sadece Windows'ta çalışır.

## Kurulum

[Releases](../../releases/latest) sayfasından `nevar_..._x64-setup.exe` dosyasını indirip çalıştır. Windows "bilinmeyen yayıncı" uyarısı verirse **Ek bilgi → Yine de çalıştır**. Yeni sürümler uygulama içinden otomatik gelir.

## Nasıl çalışır

- **Ağ:** [iroh](https://iroh.computer). Eşler public key ile bağlanır; NAT hole punching doğrudan yol açar, olmazsa iroh'un public relay'leri devreye girer. Uçtan uca QUIC/TLS.
- **Ses:** Opus (libopus), 32 kbps, 20 ms frame'ler halinde QUIC datagram'ları üzerinden. Adaptif jitter buffer, FEC ile kurtarma ve kayıp gizleme.
- **Ses işleme:** WebRTC AEC3 yankı engelleme ve AGC2 ([sonora](https://crates.io/crates/sonora) ile), RNNoise gürültü bastırma ve ses algılama ([nnnoiseless](https://crates.io/crates/nnnoiseless) ile).
- **Uygulama:** Tauri 2 + React/TypeScript. Tüm ağ ve ses kısmı Rust'ta.

## Yapı

| Klasör | İçerik |
|---|---|
| `crates/nevar-core` | Ağ, ses hattı, arkadaş/arama servisi |
| `crates/nevar-cli` | Komut satırı test aracı (ağ testi, arama) |
| `app/` | Masaüstü uygulaması (Tauri) |

## Geliştirme

Gerekenler: Rust, Node + pnpm, Visual Studio C++ Build Tools (CMake ile; bkz. `.cargo/config.toml`).

```bash
cd app
pnpm install
pnpm tauri dev
```

Testler (bazıları internet ister, gerçek node'lar başlatır):

```bash
cargo test -p nevar-core
```
