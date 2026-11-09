# Bugfix fitur: attachment gambar, navigasi, dan Council

## Scope dan bukti

Pemeriksaan dimulai dari dua screenshot yang diberikan user:

- `.aether-agent/attachments/pasted_image_20261108035430_581e9c3a.png`
- `.aether-agent/attachments/pasted_image_20261108035444_85e45b75.png`

Teks screenshot diekstrak melalui Windows OCR. Gejala yang tercantum: gambar disimpan sebagai file lokal, prompt hanya memuat path, dan model mencoba `read_file` yang memang menolak binary. Penelusuran kode mengonfirmasi tidak adanya pengiriman gambar multimodal pada jalur itu. Menonaktifkan validasi binary pada `read_file` bukan solusi; payload gambar harus dikirim melalui adapter vision.

Audit tambahan dibatasi pada batas fitur terkait: paste/drop, dispatch pesan, persistence/replay, retry/edit/undo, navigasi sesi, dan pemilihan jawaban Council. Ini bukan klaim audit atau penyelesaian seluruh bug AetherAgent.

## Temuan dan status

| ID | Tingkat | Gejala dan pemicu | Status |
|---|---|---|---|
| IMG-01 | Tinggi | Paste/upload gambar menghasilkan path teks saja; model tidak menerima piksel | Diperbaiki melalui payload `imageInputs`, hydration native, serta konversi OpenAI dan Anthropic |
| IMG-02 | Tinggi | Path canonical Windows `\\?\D:\...` yang dihasilkan penyimpanan ditolak preflight gambar | Diperbaiki; menerima local Disk/VerbatimDisk, tetap menolak UNC/device/reparse |
| IMG-03 | Tinggi | Paste/drop lambat selesai setelah pindah sesi/workspace atau clear dan memasukkan attachment ke composer yang salah | Diperbaiki dengan origin session/workspace/generation dan validasi native expected workspace |
| IMG-04 | Sedang | Send/Enter berjalan sebelum file paste selesai tersimpan | Diperbaiki: status persiapan dan pengiriman/queue ditahan sampai persiapan selesai |
| IMG-05 | Sedang | Retry/edit/undo gambar membaca ulang file yang sudah dihapus; image-only retry ditolak karena teks kosong | Diperbaiki: payload tersimpan dipakai ulang, hanya file baru dipersiapkan; teks kosong diizinkan jika masih ada attachment |
| IMG-06 | Sedang | SVG/BMP dan format tidak didukung tampak seolah telah dikirim untuk vision | Disclosure ditambahkan; tetap file context, bukan input piksel |
| NAV-01 | Tinggi | `/clear` saat `switchSession` tertunda meninggalkan navigation pending dan memblokir semua send | Diperbaiki: pending intent dibatalkan bersama generation |
| NAV-02 | Tinggi | `/undo` atau `/retry` selesai setelah New lalu membawa prompt/attachment sesi lama ke sesi baru | Diperbaiki: hasil async ditolak jika session/generation asal berubah; error dan cleanup juga origin-scoped |
| COUNCIL-01 / ST-10 | Sedang | Memilih pemenang ketika streaming menyalin jawaban parsial untuk konteks berikutnya | Diperbaiki: pilihan ditahan sampai kandidat dan parent run selesai; pending chunks di-flush sebelum penyalinan |
| COUNCIL-02 | Rendah | Tombol Use response terlihat aktif tetapi callback tidak ada atau parent belum Done | Diperbaiki: state parent diteruskan ke kartu; tanpa callback tombol tidak ditampilkan |

## Alur gambar setelah perbaikan

1. Paste, drop HTML/native, atau dialog file mengikat operasi ke composer asal. Native save memvalidasi workspace yang ditangkap sebelum operasi async.
2. App meneruskan workspace aktif ke ChatCanvas; ada tes App yang menjalankan composer asli untuk mencegah wiring ini terlewat.
3. Sebelum pesan/run dibuat, hook mempersiapkan hanya PNG/JPEG/GIF/WebP terpilih melalui IPC. Hasil yang terlambat setelah navigasi atau perubahan model/settings diabaikan.
4. Native membaca file regular secara bounded, menolak symlink/reparse dan path perangkat/jaringan, memverifikasi signature/MIME/base64 serta ukuran.
5. Payload disimpan pada pesan user sebagai `imageInputs: [{sourcePath, mediaType, data}]`; base64 tidak dimasukkan ke teks composer atau bubble.
6. IPC send dan runner memvalidasi payload sebelum provider work/run registration. Model `deepseek-chat` dan `deepseek-reasoner` ditolak dengan petunjuk memilih model vision. Model kustom lain diserahkan ke validasi provider, bukan denylist spekulatif.
7. Replay menggunakan payload tersimpan tanpa membaca file sumber. OpenAI menggunakan `image_url`; Anthropic menggunakan `image.source`. Pergantian adapter tidak membuang gambar atau signed thinking.
8. Retry/edit/undo/queue membawa payload yang cocok dengan attachment terpilih; attachment yang dibuang tidak ikut terkirim. Council menerima gambar yang sama pada kedua kandidat.

Batas per pesan: maksimal 4 gambar, 5 MiB per gambar, dan 12 MiB total decoded/raw. Ini validasi signature/MIME dan ukuran, bukan full image decoding. Data rusak yang meniru header masih dapat ditolak oleh provider.

Estimasi konteks menghitung gambar terpisah, bukan panjang base64. Angka 1.600 token per gambar adalah heuristik, bukan biaya vision model yang pasti.

## Perbaikan self-improvement yang ikut diverifikasi

Perubahan dari permintaan sebelumnya tetap dipertahankan: review agent tool-driven, interval lintas turn, update-first, fresh-read/CAS/locks, direktori global/kustom, proposal approval durable dan session-scoped, sinkronisasi keputusan ke history/cache, serta alias bare `/refine`. Detail dan batas parity dengan Hermes ada di `docs/self-improvement-review.md`. Ini bukan port seluruh memory/curator/profile/idle-queue Hermes.

Tes source-wiring yang semula sensitif terhadap whitespace `cargo fmt` diperbaiki tanpa mengubah perilaku dispatch. Pemeriksaan kini menormalisasi whitespace dan tetap memeriksa branch serta urutan dispatch yang diwajibkan.

## Verifikasi

Hasil akhir pada working tree terintegrasi:

| Pemeriksaan | Hasil |
|---|---|
| `cargo test --manifest-path src-tauri/Cargo.toml` | 282 passed, 0 failed, 2 ignored fixture subprocess; main/doc tests lulus |
| `npm test -- --fileParallelism=false` | 31 berkas, 312 passed, 0 failed |
| `npm run build` | Lulus; advisory ukuran bundle >500 kB tetap ada |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Lulus |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | Lulus |
| `git diff --check` | Lulus |

Log:

- `verification/feature-bugfix-rust.log`
- `verification/feature-bugfix-frontend.log`
- `verification/feature-bugfix-build.log`
- `verification/feature-bugfix-clippy.log`

Perintah:

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --fileParallelism=false
npm run build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Tes meliputi local HTTP/mock provider, filesystem sementara, alur storage-save → canonical path → hydration pada Windows, long path, konversi payload, replay, deleted-source resend, origin race, App wiring, navigasi, dan Council. Tes frontend menggunakan jsdom dan IPC mock; bukan pengujian desktop terpaket atau layanan provider berbayar.

## Residual yang sengaja tidak diklaim selesai

- Checkpoint streaming masih menyimpan SessionData lengkap, termasuk base64 gambar dari history. Dengan sesi bergambar besar, ini menambah biaya IPC dan disk setiap interval. Perbaikan yang disarankan adalah sidecar checkpoint cumulative untuk turn assistant, dengan guard fingerprint baseline; tidak diimplementasikan sebagai perubahan storage besar dalam batch screenshot ini.
- Format SVG/BMP/TIFF/HEIC/AVIF tidak dikonversi menjadi input vision. UI menjelaskan batasnya; SVG masih dapat dibaca sebagai teks oleh tools.
- Validasi signature bukan decoding gambar penuh; dukungan model kustom tetap bergantung pada provider.
- Tidak ada validasi live paid-provider, pengukuran prompt-cache, atau pengujian native WebView manual pada batch ini.
- Build frontend memberi advisory ukuran bundle di atas 500 kB; build tetap berhasil. Tidak ada perubahan chunking spekulatif.
- File gambar user, skill lokal yang sudah ada, dan executable distribusi tidak dihapus/ditimpa. Tidak melakukan commit, push, atau rebuild installer/executable distribusi.
