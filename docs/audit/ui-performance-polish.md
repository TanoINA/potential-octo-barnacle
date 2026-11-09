# UI/UX polish dan optimisasi terarah

## Scope

Batch ini merapikan chat, composer, navigasi, modal/menu, panel dan plan; kemudian mengurangi pekerjaan yang terbukti berulang pada startup, rendering Markdown, publikasi streaming, dan perbandingan sesi native. Perubahan screenshot/vision dan self-improvement dari batch sebelumnya dipertahankan. Tidak ada perubahan protokol provider, schema persistence, izin tools, atau penghapusan fitur.

Ini optimisasi terukur pada batas tertentu, bukan klaim seluruh aplikasi sudah optimal atau bebas bug.

## UI/UX

- Empty state berubah dari landing berisi promosi fitur menjadi prompt kerja singkat, contoh konkret, dan panduan `@`/`/` yang sudah tersedia.
- Logo orbit/diamond diganti A geometris statis. Sidebar memakai hierarki netral, selected state dengan `aria-current`, fokus keyboard jelas, dan sudut 8 px.
- Header dan composer lebih konsisten: kontrol utama 32 px, rounding 8–12 px, bayangan dan multicolor accents dikurangi. State busy/error/selection tetap terlihat tanpa ping/bounce dekoratif.
- Menu dan modal memakai permukaan sederhana; blur backdrop dan bayangan besar dikurangi pada panel terkait. Overlay dan kartu memiliki hook terpisah agar transform tidak mengubah koordinat fixed children.
- Plan menggunakan tombol checkbox yang dapat dioperasikan dengan keyboard, progressbar berlabel, warna solid dan transisi lebar 160 ms. Toggle manual tetap dipertahankan untuk step yang sama; perubahan status source tetap otoritatif.

## Motion

Definisi bersama di `src/index.css`:

| Permukaan | Durasi | Gerak |
|---|---:|---|
| Menu | 140 ms | Opacity + translateY 3 px |
| Empty state / kartu dialog | 160 ms | Opacity + translateY 3 px |
| Dimmer overlay | 140 ms | Opacity saja |
| Drawer / sidebar overlay | 180 ms | Opacity + translateX maksimal 4 px |

Animasi finite tanpa persistent fill, tanpa permanent compositor layer dan tanpa mengubah transform baris virtual. Tidak ada animasi entry per token. `prefers-reduced-motion: reduce` benar-benar mengatur `animation: none`, `transition: none`, dan scroll auto, bukan sekadar memperpendek spinner.

## Optimisasi dan hasil deterministik

### 1. Panel opsional dimuat saat dibuka

Settings, model picker, history, tools, memory, spotlight, Live Shell, artifacts library, dan artifact pane memakai React.lazy/Suspense. Sidebar dan chat inti tetap synchronous. Fallback memiliki dialog/focus trap, Escape, backdrop dan tombol Close. Chunk gagal ditahan oleh error boundary lokal sehingga chat tidak diganti halaman error global. Tidak ada tombol retry palsu untuk rejected React.lazy import yang sudah dicache.

Ukuran entry produksi, diukur dari berkas emitted dan Node gzipSync:

| | Sebelum | Sesudah | Pengurangan |
|---|---:|---:|---:|
| Entry JS | 554.458 byte | 476.879 byte | 77.579 byte / 13,99% |
| Gzip entry | 153.441 byte | 136.077 byte | 17.364 byte / 11,32% |

Entry akhir `index-DfZk_yly.js`. Optional chunks tetap ada dan diambil saat diperlukan; ini bukan penghapusan ukuran total fitur. Tidak ada warning chunk JS >500 kB pada build akhir. Tidak mengklaim waktu startup native lebih cepat dengan persentase yang sama: startup timing belum diukur.

Bukti: `verification/bundle-before-sizes.json`, `verification/ui-performance-bundle-final.json`, `verification/measure-ui-bundle.mjs`.

### 2. Publikasi streaming sesuai frame

Event text/reasoning/terminal/Council yang hanya menambah buffer kini mengantrekan RAF dan checkpoint, lalu keluar sebelum unconditional publication. Notice yang memang ada tetap dihapus segera. No-op mutator tidak memublikasikan object yang sama.

Pada fixture 200 event sebelum RAF dilepas:

- Text/reasoning: 2.400 setter invocation menjadi 0 sebelum frame.
- Terminal/Council: 1.200 menjadi 0 sebelum frame.
- Tetap satu RAF dan satu checkpoint timer; frame yang dieksekusi memublikasikan state lengkap.

Ini hitungan dispatch setter, bukan klaim 2.400 commit render. Tes juga memastikan checkpoint tanpa RAF, tool/done barriers, urutan segmen, snapshot immutable dan sesi offscreen tetap benar.

### 3. Batas memoization yang aman

ChatCanvas meneruskan callback stabil yang menerima ID; MessageRow baru mengikat ID saat interaksi. Props callback lama dipertahankan. Tidak ada custom comparator yang menyembunyikan callback baru.

MarkdownRenderer memakai memo berdasarkan string content, plugin dan renderer definitions yang stabil. Pada 10 segmen Markdown yang tidak berubah, perubahan status tool tidak lagi memicu 10 parse ulang; jumlah tambahan menjadi 0. Mengubah segmen terakhir saja memicu tepat satu parse. Composer typing tidak merender ulang dua row statis dalam fixture.

Review independen menemukan plan streaming menjadi stale setelah komponen tidak lagi remount. Itu diperbaiki lewat reconciliation text+occurrence dan source status, bukan dengan mengembalikan remount per render. Tes memeriksa step tambahan/koreksi/hilang, duplikat, toggle manual, reordering, status source, dan perubahan fence plan ke code biasa.

### 4. Perbandingan sesi tanpa dua salinan JSON besar saat perubahan sudah pasti

Guard borrowed-field pada storage membuktikan ketidaksamaan melalui counts, message content/reasoning dan artifact content. Jika berbeda, dua serialisasi pembanding dilewati. Jika sama, exact JSON comparison lama tetap dipakai untuk seluruh field lain termasuk imageInputs, providerMessages, segments, Council dan model override. Error fallback tetap dipropagasikan.

Fixture history dengan gambar 2 MiB menunjukkan 0 pemanggilan comparator exact pada enam perubahan yang bisa dibuktikan, dibanding 6 sebelum guard. Gambar dan field non-guard tetap diuji melalui fallback. Timestamp identik, metadata otoritatif, tombstone, disk roundtrip dan review merging tetap dipertahankan.

Batas: payload IPC, pembacaan sesi, dan full-file disk checkpoint masih ada; optimisasi ini mengurangi alokasi pembanding, bukan menyelesaikan residual full-session image checkpoint.

## Verifikasi akhir

| Gate | Hasil |
|---|---|
| `npm test -- --fileParallelism=false` | 40 files, 370 passed, 0 failed |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 286 passed, 0 failed, 2 ignored subprocess fixtures; main/doc tests lulus |
| `npm run build` | Lulus, termasuk TypeScript |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Lulus |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | Lulus |
| `git diff --check` | Lulus |
| Review independen scope perubahan | Tidak ada blocker; dua saran coverage, salah satunya focus handoff sudah ditambahkan |

Log: `verification/ui-performance-frontend.log`, `verification/ui-performance-rust.log`, `verification/ui-performance-build.log`, `verification/ui-performance-clippy.log`, `verification/ui-performance-whitespace.log`.

Tes fokus loading → loaded modal → close menggunakan hook aksesibilitas asli dan RAF terkontrol; focus trap kedua fase dan pemulihan opener asli lulus. Negative control yang menonaktifkan restoration menghasilkan kegagalan seperti diharapkan, lalu dikembalikan; tidak ada perubahan production untuk kasus itu.

## Browser produksi

Chromium headless menjalankan Vite preview dari build produksi dengan fixture IPC in-memory, bukan Vite dev client. Pemeriksaan pada 1280×850, 900×600 dan 390×844:

- 49 smoke checks dan 18 geometry checks lulus, total 67/67.
- Tidak ada horizontal page overflow; composer dan dialog berada di viewport.
- Kontrol toolbar/composer yang disampel tidak bertumpuk dan tidak keluar container.
- Lazy Settings chunk dimuat; Escape menutup settings dan sidebar overlay.
- Normal panel animation terukur `ui-rise` 160 ms.
- Reduced motion menghasilkan animation none dan durasi 0, termasuk spinner; tidak ada active animation.
- Native keyboard Space/Enter pada tombol checkbox plan mengubah status dan progress 33 → 67 → 33, tanpa kehilangan fokus/nama aksesibel. Diuji juga saat reduced motion.
- Tidak ada JavaScript exception, console error, network failure, HTTP error atau IPC fixture yang tidak tertangani.

Bukti akhir: `verification/ui-browser-2026-11-08T07-05-44-998Z/report.md`, `report.json`, `geometry.md`, `geometry.json`, serta sepuluh screenshot PNG. Pengukuran geometri bukan pemeriksaan artistik pixel-by-pixel; fixture browser bukan pengujian Rust IPC/WebView/provider live. Server dan Chrome QA ditutup setelah selesai.

## Batas dan langkah selanjutnya

- Belum mengukur native startup latency, FPS, peak RSS, CPU atau disk throughput pada workload produksi panjang. Perbaikan di atas membuktikan ukuran bundle dan pengurangan pekerjaan tertentu, bukan angka performa menyeluruh.
- Full history/image checkpoint tetap residual. Perbaikan sidecar/delta perlu batch storage terpisah dengan recovery/CAS test; tidak dirombak diam-diam dalam polish UI.
- Markdown yang benar-benar berubah tetap perlu diparse; sesi besar tetap bergantung pada virtualizer yang sudah ada.
- Tidak menambah dependency animasi, mengganti framework, memigrasi data, menghapus fitur, commit/push, atau membangun executable/installer distribusi. Frontend `dist` dibangun untuk pengujian; executable lama belum otomatis memuat perubahan.
