# Lazy loading riwayat sesi

## Perilaku user

- Membuka sesi mengambil **50 pesan terbaru**, bukan seluruh pesan untuk tampilan.
- Scroll sengaja ke atas dalam jarak 120 px dari batas atas mengambil hingga 50 pesan sebelumnya. Mount, scroll programatik dan posisi diam tidak memicu rentetan request.
- Tombol **Load older messages** tetap tersedia untuk keyboard atau ketika auto-load tidak cocok. Loading menonaktifkan request ganda; error tampil inline dengan Retry eksplisit.
- Header menampilkan jumlah pesan loaded/total. Timeline hanya mengacu pada pesan yang sudah dimuat.
- Posisi pesan dijaga berdasarkan message ID dan offset piksel, bukan selisih scrollHeight. Perubahan tinggi row di atas anchor tetap dikoreksi.
- Berpindah ke sesi yang belum cached memperlihatkan status loading dan menahan aksi composer, bukan mengizinkan aksi pada chat sebelumnya.
- Kembali ke sesi cached memakai data dan posisi scroll yang tersimpan. Cache dibatasi ke enam sesi inactive yang aman dievikt; sesi aktif, berjalan, dirty, atau punya operasi persistence/history pending dilindungi. Karena sesi terlindungi dipin, enam bukan batas keras seluruh pemakaian memori.
- Rename tidak mereset scroll. New/Clear membatalkan intent navigation/history lama; response terlambat tidak dipublikasikan ke sesi lain.

## Penyimpanan dan bounded read

Canonical JSON sesi tetap menjadi sumber kebenaran. Tidak ada penghapusan history atau migrasi destruktif.

`~/.aether-agent/session-pages.sqlite` merupakan projection turunan yang dapat dibangun kembali:

- metadata sesi dan model override disimpan terpisah dari row pesan/artifact;
- request warm memeriksa stat canonical lalu melakukan query ordinal berindeks untuk maksimal 100 row pesan; UI memakai 50;
- cold/invalidated projection membaca JSON canonical sekali untuk membangun projection;
- listing sesi warm membaca header metadata, bukan content seluruh chat;
- katalog yang rusak gagal dengan diagnostik, tidak pura-pura menjadi daftar kosong;
- marker `.deleted` diterapkan pada katalog, page read, full read dan semua penulisan canonical.

Page membawa artifact terbatas: maksimal 80 referenced dan 20 recent unlinked. Artifact lain dapat dibaca secara individual melalui `read_session_artifact`. Ini batas row, bukan byte: satu pesan atau artifact tetap dapat besar.

Freshness projection memakai ukuran dan mtime file. Internal writers memperbarui projection/revision secara eksplisit, sehingga penulisan internal equal-size tetap invalidated. Edit eksternal yang mempertahankan ukuran dan mtime persis berada di luar jaminan deteksi ini.

Projection refresh transactional dan incremental. Pada fixture 300 pesan + 300 artifact, perubahan satu pesan menghasilkan satu UPDATE pesan, tanpa INSERT/DELETE pesan atau mutation artifact. Row dihapus hanya bila tidak lagi ada. Seluruh row masih diserialisasi/dibandingkan saat refresh; canonical JSON tetap ditulis penuh.

## Kontrak native

```typescript
interface SessionWindow {
  offset: number;            // banyaknya prefix yang belum dimuat
  total: number;             // total canonical pada revision ini
  revision: string;          // token opaque, bukan timestamp UI
  firstMessageId?: string | null;
}
interface SessionPage {
  data: SessionData;
  window: SessionWindow;
}
```

IPC baru:

- `load_session_page({sessionId,before?,limit?,revision?})`
- `save_session_page({data,window})`
- `checkpoint_session_page({data,window})`
- `read_session_artifact({sessionId,artifactId})`
- `rename_session_summary({sessionId,title})`
- `refresh_session_window({sessionId,data,window,allowMetadata,reviewRunId?})`

`before` adalah ordinal exclusive. Pesan hasil selalu chronological. Halaman berikutnya menggunakan offset dari hasil sebelumnya dan revision yang sama.

Frontend `SessionData.historyWindow` bersifat transient. Metadata itu tidak disimpan sebagai canonical history, dan snapshot parsial tidak dikirim ke full-replacement save API.

## Perlindungan history

Partial save memvalidasi session ID, revision, offset, first-message identity, urutan suffix dan duplicate IDs sebelum menulis:

```
canonical prefix yang belum dimuat + snapshot tail yang tervalidasi
```

Native mempertahankan hidden artifact, metadata otoritatif, review result native, dan omitted image/provider payload. Partial snapshot tidak boleh memotong atau mengurutkan ulang suffix yang sudah tersimpan. Undo/edit/rollback melakukan full hydration on-demand sebelum operasi destruktif. Clear sengaja memakai full replacement, karena maksud user memang mengosongkan sesi.

Queued snapshot mempertahankan offset dan boundary asalnya. Hanya acknowledgement revisi dari penulisan sah yang dapat dipakai untuk snapshot berikutnya. Acknowledgement yang datang saat streaming sudah maju tidak memublikasikan ulang data lama.

### Native metadata dan review

Pin, rename dan review dapat mengubah revision tanpa mengubah teks history. Reconciliation memakai **last acknowledged committed snapshot**, bukan teks streaming yang belum tersimpan. Native membandingkan seluruh loaded suffix/artifact dan hanya mengizinkan perbedaan metadata yang diminta atau review dari run yang ditentukan. Ini bukan mekanisme generic stale-save retry.

Review untuk turn yang belum dimuat tetap merekonsiliasi sesi cached. Foreground review sekarang dipancarkan setelah native commit, bukan sebelum detached write. Penulisan event review identik idempotent dan tidak mengganti revision lagi.

### Konflik eksternal

Revision stale tidak diatasi dengan mengganti token berdasarkan satu boundary ID. Setelah error:

- Dirty/running/persistence-busy session mempertahankan data dan revision lokal, lalu meminta user menyelesaikan pekerjaan dan reload. Tidak ada silent rebase/save.
- Retry eksplisit pada sesi clean/idle merekonstruksi loaded suffix dari canonical melalui request bounded, kemudian memasang **content, revision, dan committed baseline secara bersamaan**.
- Snapshot canonical baseline dipisahkan dari overlay review UI. External edits dengan ID sama tidak boleh ditimpa oleh tail lama.
- Jika canonical berubah lagi saat recovery, atau user menavigasi/mengubah data, seluruh recovery dibuang tanpa meng-upgrade revision.

Arbitrary external insertion/reordering dapat memindahkan anchor di luar loaded range. Recovery mempertahankan rentang ordinal dan loaded count jika memungkinkan; bukan pencarian ID global.

## Konteks model tidak ikut terpotong

Run request paged membawa historyWindow dan pesan tampilan yang tersedia. Native memvalidasi suffix lengkap lalu merakit history dari **canonical storage**, bukan mempercayai replacement isi history dari client:

- semua ID dari offset sampai current-user harus sesuai urutan canonical;
- historical role, content, image, provider transcript, reasoning dan tool fields berasal dari canonical;
- hanya content current-user boleh memakai transformasi API seperti attachment expansion atau `/plan`;
- assistant placeholder streaming milik run yang sama tidak ditambahkan ke konteks model;
- normal dan Council sama-sama hydrate sebelum validasi gambar dan registrasi run.

Karena itu browsing tetap ringan tanpa mengurangi konteks agent. Full load masih sah untuk kebutuhan tertentu—destructive edit, export, compaction, atau native model preparation—bukan untuk membuka/scroll UI.

## Kontensi dan checkpoint

Canonical I/O memakai keyed per-session locks: penulisan sesi A tidak memegang canonical lock sesi B. Catalog melepaskan lock per file. Registry koneksi SQLite tidak memegang mutex global registry sepanjang query.

Review merge yang semula mencari ulang setiap pesan diganti indeks HashMap dengan semantik first-duplicate yang sama. Fixture 1.000 pesan turun dari 500.500 perbandingan key menjadi kurang dari 4.000.

Batas: koneksi SQLite yang sama tetap serial; rebuild cold besar masih dapat menahan koneksi. Projection refresh masih synchronous dan canonical checkpoint masih membaca/menulis JSON lengkap. Tidak ada klaim sidecar/delta checkpoint penuh atau waktu loading native tertentu tanpa benchmark perangkat asli.

## Verifikasi

Regresi mencakup warm bounded row reads, revision persistence/reopening, legacy import, rollback transaksi, stale cursor, corrupted original preservation, tombstone, prefix/artifact/image/protocol preservation, current-user transformation, cache eviction, clean switch tanpa save, streaming pending tail, baseline acknowledgements, metadata/review races, conflicting external edits, origin navigation, and measured scroll anchors.

Log final:

- `verification/pagination-rust.log`
- `verification/pagination-frontend.log`
- `verification/pagination-build.log`
- `verification/pagination-clippy.log`
- `verification/pagination-whitespace.log`

Production Chromium fixture sesi 1.000 pesan menghasilkan 29/29 checks pada build saat pengujian:

- 50 pesan awal dengan 4 row mounted;
- tepat satu request per upward intent;
- 0 px anchor drift setelah prepend, resize row 137 px dan cache return;
- recovery error eksplisit tanpa idle retry;
- rename/pin revision acknowledgement;
- same-ID canonical edit dipertahankan;
- tidak ada full `load_session`, provider execution, unknown IPC, JS exception, console/network error saat browsing.

Bukti final: `verification/session-pagination-2026-11-08T09-27-43-750Z/report.md`, `report.json` dan enam screenshot. Run ini menguji entry produksi `index-CNr7y1hF.js` setelah baseline-recovery dan navigation-cancellation fix. Harness menggunakan actual production React/TanStack di Chromium dengan IPC mock; tidak membuktikan native WebView latency atau live-provider performance.

Perintah gate akhir:

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm test -- --fileParallelism=false
npm run build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Hasil frontend final: **45 files, 433 passed, 0 failed** melalui `npm test -- --fileParallelism=false`. TypeScript noEmit juga lulus setelah perubahan test terakhir.

Hasil native final: **320 passed, 0 failed, 2 ignored subprocess fixtures**, dengan `RUST_TEST_THREADS=2` pada proses verifikasi; command Cargo tetap sama. Main/doc tests lulus. Clippy strict, Rustfmt, whitespace dan production build juga lulus.

Catatan transparansi: suite native dengan concurrency default tidak stabil pada mesin ini—run sebelumnya gagal pada elapsed-time Git/MCP/review provider fixtures. Satu run default sempat lulus, tetapi repeat tidak konsisten. Batas concurrency final mengurangi kontensi tanpa menambah timeout, mengabaikan tes, atau mengubah scheduling production. Ini bukan klaim default-parallel flakiness telah selesai.

Test skill discovery kini memakai root sementara per-process, bukan library user asli. Regresi juga menemukan starter skill description YAML yang tidak valid; template baru sudah diquote, tanpa menimpa starter user yang sudah ada. Test App lazy panel disinkronkan terhadap actual import dan teardown async; assertions loaded dialog/callback tidak dilonggarkan.

Tidak ada commit/push, instalasi, penghapusan history user atau rebuild installer yang tersirat. Installer/executable dari batch sebelumnya tidak otomatis berisi fitur ini.
