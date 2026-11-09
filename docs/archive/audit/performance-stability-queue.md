# Audit performa, stabilitas, dan antrean chat

Tanggal: 9 November 2026. Cakupan: React/Tauri. Rune tidak diperiksa atau diubah.

## Ringkasan

Perbaikan menargetkan pekerjaan berulang yang dapat diukur, alokasi berisiko, dan bug perpindahan sesi. Tidak ada migrasi format data, pemotongan history canonical, perubahan dependensi, commit, atau penggantian executable rilis.

Working tree sudah mempunyai banyak perubahan sebelum audit. Perubahan dilakukan secara terarah di atas keadaan tersebut, bukan dengan reset ke HEAD. Statistik diff terhadap HEAD bukan ukuran perubahan pekerjaan ini.

## Temuan yang diperbaiki

| ID | Severity | Lokasi | Pemicu dan dampak | Status |
| --- | --- | --- | --- | --- |
| Q-01 | Tinggi | ChatCanvas.tsx | Effect perubahan session ID mengosongkan queue ketika pindah A → B → A. | Queue, pause, dan dispatch dipisahkan per sesi. |
| Q-02 | Tinggi | ChatCanvas.tsx | Acceptance send tertunda setelah navigasi mengabaikan penyelesaian queue asal. | Acceptance/rejection menyelesaikan item pada sesi asal. |
| UI-01 | Tinggi | FilesChangedCard.tsx | Path constructor, __proto__, atau toString mengenai properti warisan dictionary dan menjatuhkan render. | Dictionary diganti Map. |
| UI-02 | Tinggi | ExecutionTrace.tsx, MarkdownRenderer.tsx, utils/encoding.ts | Lone UTF-16 surrogate menyebabkan encodeURIComponent melempar URIError saat render. | Normalisasi surrogate tidak berpasangan hanya pada metadata encoded; pasangan valid tetap utuh. |
| UI-03 | Tinggi | ChatCanvas.tsx | Paste/drop native >25 MiB dibaca dan diperbesar menjadi number array sebelum ditolak native. | Periksa ukuran sebelum membaca dan sebelum IPC. Browser placeholder tidak membaca bytes yang tidak digunakan. |
| UI-04 | Tinggi | SideBySideDiff.tsx | Satu baris minified panjang membuat span per token; virtualisasi baris tidak melindungi. | Pasangan baris >8.000 code unit melewati word highlighting tanpa memotong teks. |
| UI-05 | Sedang | ExecutionTrace.tsx | Timer induk 250 ms memetakan ulang tool dan mengulang parse argumen. | WeakMap metadata per identitas tool; clock 1 detik khusus baris aktif yang ditampilkan. |
| UI-06 | Sedang | ChatCanvas.tsx | Filter seluruh lampiran diulang untuk setiap chip pada setiap render. | Empat ID preview dihitung sekali dengan memoized Set. |
| UI-07 | Sedang | FilesChangedCard.tsx | Mount receipt review-only, termasuk kosong, memanggil git_status tanpa asal workspace terverifikasi. | Effect query Git dihapus; Review artifact dipertahankan. |
| N-01 | Tinggi | tools.rs | edit_file mengalokasikan hasil replace sebelum batas 10 MiB diperiksa; ekspansi dapat memicu OOM native. | Panjang UTF-8 diproyeksikan dengan checked arithmetic sebelum replace. |
| N-02 | Sedang | tools.rs | Diff identik dihitung dua kali untuk unified diff dan statistik. | Satu TextDiff dipakai bersama. |
| N-03 | Sedang | tools.rs | Kedua output pipe tertutup saat child hidup membuat recv(None) langsung siap dan busy-loop. | EOF menonaktifkan receive; menunggu cancellation/deadline dengan cadence maksimum 50 ms. |
| N-04 | Tinggi | runner.rs | Seluruh tool batch dimulai tanpa batas konkurensi. | Maksimal empat dispatch aktif per batch; urutan hasil dipertahankan, queued call tidak memicu efek samping setelah Stop. |
| N-05 | Tinggi | runner.rs | Integrasi cancellation sempat menimpa terminal malformed error menjadi cancelled, menggagalkan final save. | Ditemukan review dan diperbaiki sebelum selesai; tes storage/event integration memverifikasi final save. |
| S-01 | Tinggi | session_journal.rs | Seal tersimpan, canonical replacement gagal, lalu checkpoint valid berikutnya ditolak. | Sequence lebih baru hanya boleh membuka seal jika canonical masih base yang sama; committed/foreign seal tetap dilindungi. |
| S-02 | Tinggi | storage.rs | Artifact Library menahan seluruh SessionData dan gambar historis semua sesi sekaligus. | Ekstraksi per file dan drop history sebelum membaca file berikutnya. |
| S-03 | Sedang | session_pages.rs | Update content menghapus/memasukkan ulang seluruh mapping ID SQLite. | Conditional upsert dan penghapusan ID usang dalam transaksi yang sama. |
| V-01 | Sedang | ChatCanvas.tsx | Empty jump dari preview.clear saat navigasi menimpa pemulihan scroll dengan jump ke bawah. | Reset navigasi dibedakan dari explicit latest, termasuk pembatalan navigasi dengan memilih chat yang masih terlihat. |
| V-02 | Sedang | ChatCanvas.tsx | Koreksi anchor cache dapat bertahan tanpa batas dan melawan keyboard scroll ke bawah. | Pemulihan navigasi dibatasi 750 ms dan dibatalkan input; prepend tetap melindungi resize terlambat sampai input baru, tanpa polling berkelanjutan. |

## Bukti pengurangan pekerjaan

Angka fixture ini bukan benchmark FPS atau total RSS aplikasi.

| Jalur | Sebelum | Sesudah | Log di verification/performance-stability |
| --- | --- | --- | --- |
| Mapping ID, update content pada 300 pesan | 300 INSERT + 300 DELETE | 0 mutasi mapping | journal-projection-ss05-red/green.log |
| Snapshot penuh saat listing enam sesi | 6 | 1 | artifact-memory-02-red.log, artifact-memory-04-green.log |
| Poll foreground setelah EOF | 2.362.205 iterasi / 1.811 ms | 34 iterasi / 1.760 ms | foreground-01-red.log, foreground-02-green.log |
| Tool aktif pada batch 12 call | 12 | 4 | runner-concurrency-01-bounds-red.log, runner-concurrency-02-bounds-green.log |
| Parse argumen static pada tick durasi | Berulang | 0 tambahan | trace-render-red-parsing.log, trace-render-final.log |
| Seleksi preview saat mengetik dengan 80 file | 13.120 pembacaan path | Di bawah 960, batas linear pada tes | attachment-preview-cost-red.log, queue-attachments-green.log |
| Cache scroll A → B → A, Chromium produksi | Anchor hilang, viewport ke bawah | message-0949, offset -215 px → -215 px | viewport-browser-red/report.json, viewport-browser-green/report.json |

## Kontrak queue

- Queue milik session ID. Hanya queue chat yang terlihat dipertimbangkan untuk dispatch; chat tersembunyi tidak auto-dispatch.
- Loading, history preview, streaming, review, saving results, Stop, dan pending dispatch melindungi dari pengiriman tidak sesuai.
- Mode eksekusi, Council candidate, attachment, dan prepared image inputs mengikuti item asal.
- Stop/Resume per sesi. Rejection mempertahankan item dan tidak memicu retry loop.
- Acceptance setelah navigasi hanya menghapus ID asal. Remove/Edit tidak menggunakan callback sesi lama.
- Queue masih dalam memori selama canvas terpasang; persistence lintas restart/reload bukan bagian perubahan.

## Verifikasi

Semua bukti: [folder verifikasi](file:///D:/AetherAgent/verification/performance-stability).

- Regresi RED diamati sebelum perubahan perilaku, dilanjutkan GREEN terarah dan compatibility tests.
- Native final: 384 passed, 0 failed, 2 ignored; cargo test --manifest-path src-tauri/Cargo.toml --lib --locked -- --test-threads=2. Dua ignored adalah fixture subprocess yang digunakan tes induk.
- Cargo check --locked lulus; dua warning dead-code pada helper replacement storage yang sudah ada tidak dibersihkan di luar cakupan.
- Frontend/build/Chromium/Clippy: hasil akhir dicatat pada bagian Hasil final setelah proses selesai.
- Baseline native: 369 passed, 2 ignored. Baseline frontend berjalan bersamaan dengan penambahan tes RED subagent sehingga tiga prototype-path regression ikut terbaca gagal; log itu tidak dipakai untuk mengklaim baseline murni bersih.
- Independent review menemukan regresi terminal cancellation dan dua edge case viewport; semua direproduksi, diperbaiki, lalu ditinjau ulang.
- git diff --check lulus; temporary debug logging viewport sudah dihapus.

### Batas validasi

Chromium memakai production React/TanStack dan IPC fixture in-memory, bukan Rust melalui WebView2. Native tests memakai fixture terisolasi, bukan profil pengguna. Tidak dilakukan provider end-to-end berbayar, benchmark RAM/FPS desktop shipping, build installer baru, atau penggantian binary release.

## Backlog belum diterapkan

Berikut audit sumber untuk pekerjaan lanjutan, bukan klaim sudah diperbaiki atau direproduksi penuh. Kebijakan retensi/desain besar tidak disisipkan diam-diam.

| Prioritas | Area | Risiko / tindak lanjut |
| --- | --- | --- |
| Tinggi | Streaming tool arguments | Salinan/pengiriman seluruh prefix argumen berulang; perlu coalescing/delta IPC, byte-budget test, final flush. |
| Sedang | Task registry/log daemon | Batas agregat/retensi belum ada meski per task berbatas; harus melindungi task aktif. |
| Sedang | Hapus sesi pemilik daemon | Perlu konfirmasi/penolakan agar daemon tidak kehilangan akses pengelolaan. |
| Sedang | MCP process tree | Uji descendant-process ketika initialize dibatalkan/drop wrapper dan ownership lintas platform. |
| Sedang | Idle index | Batas rebuild belum membatasi total freshness scan; perlu continuation agar tidak starvation. |
| Sedang | Journal warm page | Sidecar aktif dapat dibaca/divalidasi dua kali; kurangi read tanpa melemahkan checksum/guard. |
| Sedang | Abandoned recovery | Fixture tambahan lifecycle recovery dan Clear/Undo setelah restart. |
| Sedang | Timeline / Live Shell | Budget tampilan dan isolasi render untuk banyak loaded turn/output; jangan memangkas canonical. |
| Rendah | Clipboard/menu/preview | Beberapa rejection, indikator copy, dan retry preview sementara memerlukan hardening terpisah. |
| Rendah | Bundle awal | Warning chunk >500 kB tetap ada; jangan menyembunyikan dengan menaikkan warning limit. |

## Invariant yang dipertahankan

- Hidden prefix, artifact canonical, image/provider payload, dan konteks model tidak dipotong.
- Tombstone mengalahkan leftover JSON; corruption diagnostics tetap fail-visible.
- Duplicate/empty ID rollback seluruh transaksi projection.
- Tool terminal durable tidak boleh diganti status terminal lain.
- Limit konkurensi per batch foreground, bukan global semua sesi. Tool loop subagent tetap serial; tidak ada shared semaphore baru.
- Artifact listing masih parse satu file sesi penuh; hasil artifact tetap ditahan. Ini bukan streaming JSON parser atau artifact-only index.

## Berkas pekerjaan

19 berkas source/tes: ChatCanvas.tsx dan tes commands/attachments/pagination; ExecutionTrace, FilesChangedCard, SideBySideDiff, MarkdownRenderer beserta tes; utils/encoding dan tes; native tools.rs, runner.rs, storage.rs, session_journal.rs, session_pages.rs.

Harness verification/session-pagination-browser.mjs ditambah respons fixture warm_session_index yang sudah ada pada aplikasi. Assertion page budget, anchor drift, unknown IPC, error, dan resource tidak dilonggarkan.

Manifest final: [source-manifest.json](file:///D:/AetherAgent/verification/performance-stability/source-manifest.json).

## Hasil final

- Native: **384 passed, 0 failed, 2 ignored**. [Log native](file:///D:/AetherAgent/verification/performance-stability/final-native.log).
- Native compile: **lulus**. [Cargo check](file:///D:/AetherAgent/verification/performance-stability/final-native-check.log).
- Native Clippy: **lulus dengan dua warning dead-code yang sudah ada**, bukan mode -D warnings. [Log Clippy](file:///D:/AetherAgent/verification/performance-stability/final-clippy.log).
- Frontend full suite sebelum enam edge-case terakhir: **621 passed**. Rerun seluruh suite terhadap source final masih berjalan; hasilnya akan dicatat setelah selesai.
- Source final, queue + viewport focused: **52 passed**. [Log terarah](file:///D:/AetherAgent/verification/performance-stability/final-viewport-queue.log).
- TypeScript + Vite production build: **lulus**, warning chunk utama >500 kB tetap dilaporkan. [Log build](file:///D:/AetherAgent/verification/performance-stability/final-build.log).
- Chromium production, 1.000 pesan: **29 checks passed, 0 failed**, enam screenshot. Prepend, delayed resize, dan cache return masing-masing **0 px drift**. [Laporan browser final](file:///D:/AetherAgent/verification/performance-stability/browser-final-verified/report.md).
- Run browser antara yang gagal tetap disimpan sebagai bukti RED. Timer yang semula membatasi semua koreksi menyebabkan delayed prepend resize bergeser 137 px; source final membatasi timer hanya untuk pemulihan navigasi, bukan lifetime anchor prepend. Fixture warm_session_index ditambahkan tanpa melonggarkan assertion.
- Independent review akhir: tidak menemukan blocker dalam perubahan yang ditinjau.
- Preview server verifikasi sudah dihentikan. Binary release tidak diganti dan tidak ada commit/push.
