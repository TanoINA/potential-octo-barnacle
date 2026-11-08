# Perbaikan lifecycle background task — RT-02/03/07/09

## Scope dan hasil

Implementasi hanya mengubah `src-tauri/src/agent/tasks.rs`; dokumen ini adalah deliverable pendamping. Tidak mengubah runner/provider/tools/storage/lib/types/frontend dan tidak membuat commit. Schema `TaskInfo` serta API publik tetap; ID sekarang `task_<UUID v4>`. Daemon memakai token independen dan tidak dibunuh saat run/manager selesai.

Empat kategori prioritas yang ditangani:

1. **RT-02 — stdin backpressure / deadlock.** Sender dan cancellation token di-clone dalam lock pendek. Semua channel await di luar guard; input maksimal 64 KiB, antrean 32 pesan, deadline enqueue 2 detik. Writer stdin bisa dibatalkan ketika menunggu antrean maupun ketika write/flush tersumbat. Respons menyebut **queued**, bukan mengklaim delivered. Monitor abort dan join writer sebelum mempublikasikan completion.
2. **RT-03 — output tanpa newline / kuota.** Reader byte 4096, channel 32 chunk, decoder UTF-8 incremental dengan maksimal tiga byte pending; invalid byte menjadi replacement character, error baca menjadi pesan eksplisit. Ring buffer maksimal 500 fragmen dan 64 KiB/task, termasuk respons status/list. Log maksimal 1 MiB dengan marker truncation, terus menguras pipe setelah kuota tercapai. Write/flush log punya deadline 2 detik; kegagalan disimpan sebagai error monitor dan pesan output, status akhir failed, bukan diabaikan. Tail memilih fragmen bounded (baris raksasa dipotong), bukan menjanjikan baris arbitrarily besar.
3. **RT-07 — EOF/wait/cancel dan inherited pipe.** EOF hanya men-disable cabang recv. `child.wait()` dan cancel tetap berada dalam select luar. Sesudah exit, PID langsung diretire, drain maksimal 500 ms lalu reader dihentikan dan di-join; descendant yang menahan pipe tidak membuat task running selamanya. Monitor saja yang memiliki Child dan mengeksekusi kill. Windows taskkill /T /F asynchronous dengan deadline 8 detik; error/timeout dipropagasikan, direct-child kill/reap punya deadline 3 detik. `kill_task` menunggu completion atau error/deadline 15 detik; tidak menandai killed sebelum proses direap. Bila termination gagal dan child masih hidup, monitor tetap memegang dan mem-poll child, tidak mengklaim completed atau menghapus PID hidup.
4. **RT-09 — ID/log collision dan silent IO failure.** UUID menggantikan counter per-manager. Log dibuka `create_new` sebelum spawn; direktori/open gagal menggagalkan start secara eksplisit, existing log tidak di-append/overwrite. Spawn gagal mencoba membersihkan log kosong dan menyertakan error cleanup. Root log tests terisolasi di dedicated temp UUID; tidak menyentuh `.aether-agent` pengguna. Dua manager diuji memakai root log yang sama.

## Bukti RED → GREEN

RED regression ditambahkan sebelum implementasi. Percobaan focused test repo sempat terblokir perubahan concurrent di file milik worker lain: `provider_audit_tests.rs` memanggil helper private, kemudian runner sementara kehilangan `append_history_segment`. Tidak memperbaiki file di luar ownership.

Alternatif RED: scratch harness Cargo mengimpor **file tasks.rs yang sama**, dependencies nyata, dan shim `create_hidden_command`; bukan salinan implementasi atau hasil sintetis. Perintah:

```text
cargo test --offline --manifest-path C:/Users/Tano/AppData/Local/hermes/cache/scratch/aether-task-harness/Cargo.toml --target-dir D:/AetherAgent/src-tauri/target agent::tasks::tests -- --test-threads=1
```

Hasil RED: **2 passed, 5 failed, 1 ignored**. Failures konkret mencakup read guard tertahan saat antrean penuh, ID dua manager sama-sama `task_1`, output completion timeout, EOF fixture dengan PID tidak diretire, serta existing completion test timeout. RED EOF melalui shell membuktikan retirement assertion, bukan reproduksi EOF deadlock murni; test GREEN diperketat memakai child fixture langsung dan menunggu kedua pipe benar-benar EOF sebelum cancel. Fixture output/ID juga diarahkan langsung ke executable test agar tidak bergantung startup PowerShell.

Setelah integrasi concurrent kembali buildable, focused gate repo asli dijalankan, bukan hanya harness:

```text
cargo test --manifest-path src-tauri/Cargo.toml agent::tasks::tests -- --test-threads=1
```

Hasil akhir: **13 passed, 0 failed, 1 ignored, 156 filtered out**, 49.22 detik. Satu ignored adalah fixture subprocess yang sengaja hanya dipanggil eksplisit. Main target: 0 tests, sukses. Peringatan berada di git/provider/storage, bukan tasks.

Regressions mencakup: saturated-channel lock, real pipe/channel backpressure dan concurrent send/kill, input oversize, no-newline flood 2 MiB dan kuota log/IPC, UTF-8 split/invalid/incomplete EOF, collision create_new dengan isi lama utuh, direktori log tidak tersedia, injected writer failure, marker truncation UTF-8, EOF-before-exit cancellation, parent exit dengan descendant pemegang pipe, daemon survive scope sampai explicit kill, serta existing spawn/kill/not-found.

Format/whitespace: `rustfmt --edition 2021 --check src-tauri/src/agent/tasks.rs` dan `git diff --check -- src-tauri/src/agent/tasks.rs` exit 0. Tidak menjalankan full suite/build/clippy karena gates tersebut milik parent.

## Kendala dan sisa batas

- Startup PowerShell pada host sempat melebihi 15 detik, dibuktikan probe nyata (~15.7 detik); test existing completion diberi headroom 45 detik. Timeout kill tetap bounded dan kegagalan tree-kill terlihat, bukan disembunyikan. Beberapa tool patch timeout setelah write sebenarnya berhasil; diff dan focused test digunakan untuk memeriksa hasil nyata.
- Belum ada Windows Job Object atau Unix process-group enforcement. Windows taskkill success diperiksa dan direct child direap, tetapi tree tiga tingkat/reparent race serta app-exit recovery belum dibuktikan. Descendant fixture drain sengaja self-expire empat detik; tidak membunuh semua background process.
- Caller cancellation token dari parent tool belum diteruskan karena scope melarang tools/runner/API change. Enqueue tetap selesai dalam dua detik atau segera saat task cancel; daemon tidak ikut abort parent secara diam-diam.
- Kuota ini per task, bukan total registry/task-count/disk-retention global. Owner/session isolation (RT-06) tidak ditangani.
- Test `real_stdin_backpressure...` menerima tree-kill failure hanya jika error eksplisit, lalu tetap memeriksa monitor completion; existing `test_kill_running_task` mensyaratkan kill sukses dan lulus pada gate akhir.

## Self-check

| Sumbu | Nilai | Bukti/gap |
|---|---:|---|
| Accuracy | 4/5 | Gate repo nyata lulus; tree lintas platform tidak diklaim terverifikasi penuh. |
| Completeness | 4/5 | Empat kategori tercakup; parent-token wiring dan Job Objects di luar scope. |
| Clarity | 4/5 | Policy quota/queued/daemon eksplisit; tail berbasis fragmen perlu diperhatikan caller. |
| Actionability | 4/5 | Source dan perintah gate tersedia; full gates/commit tetap tugas parent. |
| Conciseness | 4/5 | Laporan memisahkan hasil dan residual; detail RED diperlukan untuk membatasi klaim. |

Penilaian keseluruhan 4.0/5. Perbaikan berikut bernilai tertinggi: process-tree regression terkontrol tiga tingkat, lalu wiring cancellation caller tanpa mengubah semantics detached daemon. Self-check: tidak menyebut full audit/runtime parity selesai hanya dari focused tests ini.
