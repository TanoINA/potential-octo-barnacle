# Perbaikan tools, Git, dan MCP

## Hasil

Dibaca seluruh 20 temuan pada `layer-provider-tools-review.md`. Perubahan subagent dibatasi pada `tools.rs`, `git.rs`, `mcp.rs`, serta penyesuaian fixture `search.rs`; tidak mengubah provider/runner/types/storage/lib/frontend/manifest dan tidak membuat commit.

- **P01:** `files_changed.path` dihitung terhadap workspace canonical yang sama, separator `/`, tanpa fallback basename. Regression Windows membedakan root `config.ts` dan nested `src/config.ts` untuk write dan edit. Konversi relative pada write dilakukan sebelum mutasi.
- **P02 (writer aplikasi):** registry mutex async per canonical path, process-wide untuk semua subagent; guard mencakup pembacaan awal sampai atomic write. Weak entries dibersihkan dan Windows key dibuat case-insensitive. Regression membuktikan edit menunggu lock sebelum membaca, membaca snapshot setelah lock dilepas, dan dua edit berbeda sama-sama bertahan. **Tidak menyelesaikan TOCTOU editor eksternal antara recheck dan rename**, hardlink identity, atau pertukaran reparse point. Komentar lama yang mengklaim perlindungan eksternal penuh sudah dikoreksi.
- **P10:** stage/revert memakai resolver workspace, menolak kosong/root/directory/traversal/magic, dan semua operasi per-file memakai `--literal-pathspecs`. Deleted target menggunakan validasi ancestor resolver. Disposable Git repo regression `[ab].txt` vs `a.txt` membuktikan hanya selected file di-stage/revert; traversal subworkspace dan absolute drive/UNC luar ditolak. Fixture menetapkan `core.autocrlf=false` agar byte assertion independen dari konfigurasi Git mesin.
- **P16:** branch/status/prefix/revert-status exit nonzero menjadi error dengan stderr maksimum 4096 karakter; spawn Git gagal dibedakan dari not-a-repo. Disposable corrupt index sebelumnya menghasilkan Working tree clean; sekarang error.
- **P17:** status produksi memakai `--porcelain=v1 -z`, parser NUL membaca tambahan record rename/copy, menjaga spasi/Unicode, mengembalikan `oldPath`, dan mengonversi nama repo-relative menjadi workspace-relative. Status subfolder dibatasi scope `.`. Parser display lama hanya dikompilasi untuk existing tests, bukan jalur produksi.
- **P08:** reader MCP chunked 8192 byte dengan limit frame 1 MiB (buffer maksimum limit + satu chunk), EOF parsial error; frame terlalu besar menggagalkan pending requests dan mematikan direct child. Request deadline 20 detik mencakup stdin mutex, write, flush, response. Request timeout menutup transport dan menjelaskan side effect indeterminate. Notification write juga bounded timeout. Reader JoinHandle disimpan/diabort saat shutdown atau last-client drop. Pending entry memakai RAII sehingga future drop membersihkannya. Publikasi result text UTF-8-safe dibatasi 200000 byte + marker. Test blocked stdin memakai parameter 800000 byte dan deadline internal 150 ms, dengan outer watchdog.
- **P13:** tools/list mengikuti cursor; cycle/malformed cursor error; maksimum 64 halaman, 4096 tools, 8 MiB katalog. Dua halaman diuji melalui manager hingga alias halaman kedua benar-benar callable.
- `search.rs`: hanya menambahkan `provider_messages: None` pada dua fixture agar cocok dengan schema yang diubah worker provider. Tidak mengklaim P19 diperbaiki.

## Verifikasi aktual

Perintah terakhir sesudah rustfmt, seluruhnya exit 0:

```sh
rustfmt --edition 2021 src-tauri/src/agent/tools.rs src-tauri/src/agent/git.rs src-tauri/src/agent/mcp.rs src-tauri/src/agent/search.rs
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::git::tests:: -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::mcp::tests:: -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::tools::tests:: -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib agent::search::tests:: -- --test-threads=1
```

| Filter | Passed | Failed |
|---|---:|---:|
| Git | 8 | 0 |
| MCP | 14 | 0 |
| Tools | 21 | 0 |
| Search fixture compatibility | 5 | 0 |
| **Total** | **48** | **0** |

`git diff --check` pada keempat source file exit 0. Ada warning konversi LF/CRLF Git, bukan whitespace error. Tidak menjalankan full build/full suite/clippy/frontend gates; parent pemilik gates pusat.

RED teramati: nested basename salah; stage glob juga memilih neighbor; traversal diterima; corrupt index dilaporkan clean; rename Unicode/subfolder rusak; lock helper belum ada; request timeout seam belum ada; result publication tanpa cap; last-client drop meninggalkan child. Sebagian percobaan RED awal terhalang compilation work-in-progress worker lain (provider/runner/storage). Regression pagination/flood ditulis sebelum implementasi tetapi tidak memperoleh behavioral RED karena blocker compilation tersebut; jangan mengklaim seluruh coverage memiliki RED runtime. Test blocked stdin pernah gagal outer watchdog ketika banyak proses test paralel; rerun isolated dan final serial lulus.

## Batas / pekerjaan tersisa

- **P14 parsial, bukan selesai:** pending RAII, managed reader, direct-child drop cleanup ada. Belum ada protocol `notifications/cancelled`, Windows Job Object/process-tree lease, bukti child/grandchild cleanup saat future connect dibatalkan, atau jaminan side effect rollback. Future drop saat partial stdin write masih dapat meninggalkan framing transport tidak reusable jika client lain tetap hidup. Jangan otomatis retry timeout sebagai operasi aman.
- P08: `taskkill` dan direct `child.kill` shutdown diberi timeout 2 detik masing-masing; menunggu mutex child sebelum shutdown dan reader-error `kill()` sendiri belum punya hard deadline. Test membuktikan direct process, bukan seluruh descendant tree. Frame cap juga membatasi result JSON masuk, tetapi tidak ada pengukuran peak allocator. Notification timeout belum otomatis menutup client yang masih dipakai.
- P01 belum fixture UNC nyata/case-varian/8.3; tidak mengklaim parity penuh seluruh alias Windows. Lock aplikasi tidak menyatukan hardlink path berbeda dan resolver missing target lewat canonical ancestor alias belum sepenuhnya dinormalisasi.
- P15 foreground descendant ownership, P18 bounded/cancellable Git commit/push, P19 FTS >10000 rows aggregation, P20 DNS pinning **belum dikerjakan**. P09 dimiliki worker tasks; provider P03–P07/P11/P12 dimiliki parent/worker provider.
- Parser NUL menolak nama non-UTF-8 secara eksplisit; legacy parser text hanya test-only. Rename dari luar workspace tidak mengekspos `oldPath` di luar scope.

## Evaluasi ringkas

| Aspek | Skor | Bukti / peningkatan |
|---|---:|---|
| Akurasi | 4/5 | 48 tests lulus; belum seluruh descendant/UNC diuji. |
| Kelengkapan | 3/5 | Prioritas filesystem/Git/MCP selesai terbatas; P14/P15/P18/P19/P20 tetap dicatat. |
| Kejelasan | 4/5 | ID temuan dan batas klaim eksplisit; laporan masih cukup panjang. |
| Actionability | 4/5 | Source coherent dan focused gates lulus; parent tetap menjalankan gates integrasi. |
| Keringkasan | 4/5 | Ringkasan final dipisahkan dari evidence detail ini. |

Peningkatan prioritas: ownership process tree, handle-bound filesystem mutation untuk race eksternal, lalu bounded Git subprocess. Penilaian sengaja tidak menganggap scope tertunda sebagai terselesaikan. Tidak menambah skill global pada pekerjaan subagent berscope file ini.
