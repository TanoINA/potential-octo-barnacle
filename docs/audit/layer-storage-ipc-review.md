# Audit lapisan persistence, IPC, settings, dan skills

## Status dan batas bukti

Audit **statis/read-only terhadap implementasi** di `D:/AetherAgent`. Satu-satunya berkas yang ditulis oleh reviewer ini adalah laporan ini. Tidak menjalankan aplikasi, build, test, lint, provider, MCP, atau perintah IPC yang mengubah state. Tidak membuka konfigurasi pengguna, database percakapan, kredensial aktual, maupun laporan `docs/audit-evidence-tools-2026-11-08.md`.

Bukti berasal dari pembacaan kode bernomor baris, inventaris IPC dengan Python, serta `git status`/`git diff --stat`. Inventaris menemukan **43 command IPC**. Tidak ditemukan `AGENTS.md` pada jalur ancestor yang diperiksa maupun daftar tracked repository. Pembacaan batch melalui `read_file` sempat timeout; alternatif Python read-only berhasil. Salah satu terminal mengeluarkan isi yang diminta tetapi timeout saat terminasi; tidak diperlakukan sebagai hasil test.

Worktree awal: laporan existing di atas berstatus untracked. Saat pemeriksaan akhir, `package.json` dan `vitest.config.ts` juga berubah dari proses lain; reviewer ini tidak menulis keduanya. Jangan mengatribusikan perubahan tersebut pada audit ini.

**Arti severity:** Tinggi = potensi kehilangan data/eksekusi atau boundary izin yang tidak sesuai janji UI; Sedang = state salah, kebocoran bersyarat, atau kegagalan operasi penting; Rendah = parity/presentasi/defense-in-depth. Semua urutan reproduksi di bawah adalah **regression scenario yang diturunkan dari source, belum dieksekusi**.

## Coverage dan alur yang ditelusuri

| Area | Bukti utama | Hasil penelusuran |
|---|---|---|
| Startup/entry/config | `src-tauri/src/main.rs:1-6`, `build.rs:1-3`, `tauri.conf.json:1-40`, `capabilities/default.json:1-13`, manifests | Entry delegasi ke library; build script standar. CSP nonaktif. Capability main lokal; tidak ada remote capability pada berkas yang diperiksa. |
| IPC | Seluruh `src-tauri/src/lib.rs:1-941` | 43 command dan registrasinya; operasi sesi, settings, workspace, skills, provider/MCP, git/shell, attachment, open/reveal/terminal, task. Implementasi internal proses/provider milik lapisan lain, hanya call boundary dibaca di sini. |
| Session storage | Seluruh `agent/storage.rs:1-1375` | Safe ID → path; legacy parse; list/load/save/rename/pin/delete/compact/export; atomic write; preview/timestamp; reflection merge dan indexing. |
| Ordering frontend | `src/hooks/useAgent.ts:180-230,324-371,565-617,657-685,767-882,915-1209,1274-1539` | Save diantrekan, sebelum run/export/switch di-await; delete menunggu queue yang sudah ada. Operasi lain tidak semuanya masuk queue yang sama. |
| Run registry | `agent/runner.rs:654-744,1296-1328,1599-1648` dan IPC spawn | Registry mencegah dua run per sesi; abort hanya meminta cancel; finish sesudah future run selesai; reflection punya lease terpisah. |
| Settings | `storage.rs:46-185`, `types.rs:54-187`, `useAgent.ts:1018-1087`, bagian relevan SettingsModal | Migrasi provider kind, validasi save, normalisasi sebagian load, optimistic update, provider/MCP secret representation. |
| Skills | Seluruh `agent/skills.rs:1-497`, `lib.rs:394-415`, call sites runner/tools | Workspace lebih dahulu, configured roots lalu global dan kompatibilitas Gemini; metadata/parser; activation read; global save; starter initialization. |
| Schema parity | Seluruh `agent/types.rs` vs `src/types/agent.ts` | Field inti sesi, council, reflection, segment, settings/request/event dibandingkan; perbedaan ada di notice, nullability dan permissive strings/value. |
| Workspace/security | `lib.rs:156-178,185-329,345-405,513-529,553-811`, `storage.rs:910-1075`, `tools.rs:1510-1601`, MarkdownRenderer link | Primary resolver mengikat workspace, tetapi export/attachment/skills/opener punya kebijakan berbeda; tidak menyamakan capability plugin dengan pembatasan custom Rust IPC. |

## Temuan prioritas

### F01 — Tinggi: mode Request Review bukan enforcement izin native

- **Lokasi:** `src-tauri/src/agent/types.rs:110-122,160-161`; `agent/runner.rs:800-806,1179-1243`; `agent/tools.rs:290-319`; `src-tauri/src/lib.rs:422-447,460-510`.
- **Trigger/call path:** pilih `request_review` → request `executionMode` → hanya menambahkan kalimat ke system prompt. Dispatcher tetap memanggil write/edit/run/MCP tanpa token approval; `ToolContext` tidak menerima execution mode. Subagent juga memakai dispatcher yang sama.
- **Dampak:** model yang mengabaikan instruksi, salah menginterpretasi persetujuan, atau terkena prompt injection tetap dapat mengeksekusi operasi. Label “Every command ... requires explicit operator approval” tidak didukung gate native. Ini bukan bukti bypass otomatis dari web luar; boundary yang gagal adalah janji approval di dalam aplikasi.
- **Fix minimal:** keputusan approval native per operasi, identitas request dan argumen terikat pada persetujuan, diteruskan ke subagent/MCP; tolak eksekusi write/execute sebelum approval. Jangan mengandalkan string system prompt.
- **Regression proof:** provider fixture mengembalikan tool `write_file`, `run_command`, `learn_skill`, MCP pada Request Review; sebelum approve tidak boleh ada side effect; deny tidak mengeksekusi; ubah argumen setelah approval harus invalid.

### F02 — Tinggi: isi run panjang hanya di RAM sampai selesai

- **Lokasi:** `src/hooks/useAgent.ts:403-475,565-617,831-844,869-882`; `src-tauri/src/lib.rs:33-36`; `agent/runner.rs:1296-1328`.
- **Trigger/call path:** `sendMessage` menyimpan placeholder assistant kosong → `send_message` spawn → chunk/tool/artifact mengubah React state tanpa checkpoint disk → save berikutnya baru di `done` atau navigasi/aksi tertentu.
- **Dampak:** crash, reload renderer, atau tutup aplikasi di tengah run panjang meninggalkan placeholder kosong di disk, menghilangkan hasil, artifact dan jejak tool yang sudah diterima. Run native tidak memiliki durable transcript sendiri. Restore menandai placeholder cancelled, tetapi tidak memulihkan konten yang belum disimpan.
- **Fix minimal:** checkpoint debounced dengan batas waktu/byte dan final flush; idealnya append/checkpoint native per run agar renderer bukan satu-satunya pemilik transcript. Gunakan revision agar checkpoint lama tidak mengalahkan clear/delete.
- **Regression proof:** kirim chunk + tool result, sebelum done lakukan simulated renderer loss/restart; reload wajib membawa progres terakhir dengan run ditandai interrupted, bukan assistant kosong.

### F03 — Tinggi: startup/switch bisa mengirim run ke workspace lama ketika restore workspace gagal

- **Lokasi:** `src/hooks/useAgent.ts:349-369,966-995`; `src-tauri/src/lib.rs:25-34,345-374,831,857-859`.
- **Trigger/call path:** backend punya cwd B dari startup/previous session; sesi A menyimpan folder yang sudah dihapus → frontend memasang data A dan workspace A terlebih dahulu → `set_workspace_cwd(A)` gagal canonicalize sebelum mengganti backend → startup menelan error; switch menampilkan error tetapi tetap meninggalkan sesi A aktif → send mengambil cwd B dari AppState.
- **Dampak:** tool dapat berjalan pada project B sementara UI menampilkan sesi/workspace A. `AgentRunRequest` tidak mengikat workspace sesi, sehingga registry run tidak mendeteksi mismatch.
- **Fix minimal:** publish sesi/workspace hanya setelah native transition berhasil; saat gagal kosongkan/nonaktifkan backend workspace dan blok send sampai diselesaikan. Sertakan workspace identity/revision dalam request, validasi native saat begin.
- **Regression proof:** backend B valid, load sesi A dengan cwd hilang, lalu send; harus ditolak atau no-workspace, tidak boleh mengeksekusi di B. Uji startup dan switch.

### F04 — Tinggi: native opener menerima konten model lalu mengirimnya ke `cmd /C start`

- **Lokasi:** `src/components/MarkdownRenderer.tsx:18-23,60-78`; `src-tauri/src/lib.rs:553-609,611-658`.
- **Trigger/call path:** klik Markdown link non-web dari hasil model → URL didecode frontend lalu native → target tidak perlu exists/berada di workspace → `Command::new("cmd").args(["/C", "start", "", &path_str])`. Tidak ada penolakan metachar shell ataupun ekstensi executable; path sederhana yang memuat `&` adalah input bagi parser cmd, bukan API open-file langsung.
- **Dampak:** risiko command interpretation dari target buatan model/repository setelah klik operator; membuka `.cmd/.bat/.exe/.lnk` pun tidak meminta approval tambahan. **Belum dilakukan exploit Windows**, sehingga bentuk payload dan quoting final perlu regression pada Windows, bukan diklaim telah terbukti dieksekusi.
- **Fix minimal:** gunakan API opener/ShellExecute tanpa command shell; allowlist scheme, validasi file exists dan kebijakan executable/shortcut, tampilkan konfirmasi untuk launch executable. Jangan menganggap `Command::args` otomatis aman bila child-nya `cmd /C`.
- **Regression proof:** target fixture dengan `&`, `%`, `^`, tanda kutip, spasi, encoded metachar, absolute/UNC dan shortcut; hanya target literal yang boleh dibuka dan tidak ada second command. Gunakan stub launcher tanpa payload destruktif.

### F05 — Sedang: mutex sesi menserialisasi I/O, tetapi snapshot usang tetap dapat mengembalikan delete/rename/unpin

- **Lokasi:** `agent/storage.rs:11,367-369,410-417,424-474,507-541`; `src/hooks/useAgent.ts:218-229,1089-1135`; `src-tauri/src/lib.rs:829-860`.
- **Trigger/call path:** A memegang snapshot, B delete/rename/unpin lebih dulu, lalu A `save_session`. Save tidak memakai expected revision/tombstone; load sebelumnya gagal setelah delete dianggap create baru. Untuk pin: snapshot pinned=true/pinnedAt=Some → unpin menyimpan false/None → snapshot lama disimpan dan `merge_pin_state` tidak menolaknya.
- **Dampak:** sesi terhapus hidup kembali, judul baru tertimpa, atau pin yang dilepas kembali aktif. Queue frontend melindungi urutan save milik satu hook dan backend lock melindungi transaksi dalam satu proses, **bukan** konflik semantik semua command atau beberapa instance. Tidak ada singleton/file lock pada startup yang diperiksa.
- **Fix minimal:** revision/CAS per sesi dan tombstone delete; mutasi metadata terpisah dari payload percakapan. Masukkan mutation ke satu scheduler dan cegah multi-instance writer bila belum didukung.
- **Regression proof:** simpan snapshot lama sesudah delete/rename/unpin, harus conflict dan tidak mengubah state baru. Jalankan dua writer/process fixture; test merge pin existing hanya menguji authoritative=false/true terhadap disk lama, bukan late stale save sesudah unpin (`storage.rs:1134-1157`).

### F06 — Sedang: compact berulang menghapus checkpoint sebelumnya

- **Lokasi:** `agent/storage.rs:570-601,637-690`.
- **Trigger/call path:** compact empat pesan → checkpoint system + turn user terakhir. Tambahkan turn baru → compact lagi. `deterministic_summary` mengolah user/assistant/tool saja, tidak checkpoint system yang ikut `messages_to_summarize`.
- **Dampak:** ringkasan tujuan/kesimpulan lama hilang saat compact kedua; local checkpoint bukan context-preserving secara rekursif. Preamble assistant juga tidak masuk summary.
- **Fix minimal:** sertakan checkpoint terdahulu secara eksplisit dengan budget yang ditetapkan, gabungkan prose dari segment/preamble dan final content, hindari merangkum teks checkpoint sebagai instruksi berprivilege baru tanpa pembatasan.
- **Regression proof:** goal unik pada turn pertama; compact, tambah dua turn, compact lagi; goal/checkpoint semula masih ada dan dipakai pada replay provider. Test existing hanya compaction pertama dan streaming rejection (`storage.rs:1195-1235`).

### F07 — Sedang: preamble/segments tersimpan, tetapi replay dan ekspor tidak merekonstruksi narasi lengkap

- **Lokasi:** `src/hooks/useAgent.ts:433-460`; `agent/types.rs:303-315`; `agent/runner.rs:1767-1790`; `agent/storage.rs:570-601,720-745,765-795`.
- **Trigger/call path:** assistant mengirim teks sebelum tool → frontend memindahkan `content` ke `preamble` dan mengosongkan content; final bisa kosong pada turn tool-only. Save menjaga fields tersebut, tetapi `append_history_message` hanya membaca `message.content` dan flattened tool_calls. Markdown/HTML juga membaca content/reasoning tanpa preamble/segments.
- **Dampak:** narasi yang terlihat di UI dan ada di JSON hilang pada provider turn berikutnya/ekspor; ekspor Markdown/HTML juga menghilangkan args/result tool, attachments, council detail, dan status. Export “transcript” tidak sama dengan isi sesi lengkap. JSON mempertahankan field Rust sehingga merupakan format lebih lengkap, bukan jaminan redaksi.
- **Fix minimal:** satu fungsi canonical transcript untuk ordered segments/preamble/final, dengan tool-call/result pairing stabil. Gunakan dalam replay/summary/export; dokumentasikan jika ekspor sengaja ringkas, jangan label sebagai full transcript.
- **Regression proof:** fixture preamble → tool → interleaved text → final dan fixture tool-only; roundtrip save/load lalu replay/export harus membawa teks sekali, urutan benar, tool pairing lengkap.

### F08 — Sedang: perubahan workspace tidak langsung dipersist ke sesi; clear workspace mengaku sukses pada kegagalan disk

- **Lokasi:** `src/hooks/useAgent.ts:1176-1201`; `agent/storage.rs:937-955`; `src-tauri/src/lib.rs:345-374`.
- **Trigger/call path:** pilih/clear workspace pada sesi existing lalu tutup sebelum send/rename/export/switch; mutasi data sesi tidak memanggil persistData. File workspace global mungkin berubah, tetapi startup frontend memilih cwd dari file sesi. Selain itu `clear_workspace` mengabaikan hasil remove_file dan selalu Ok.
- **Dampak:** workspace lama kembali setelah restart atau setting global dan sesi bertentangan; pada permission/sharing error clear dilaporkan berhasil padahal pointer disk masih lama. Ini memicu mismatch F03 bila folder lama tak tersedia.
- **Fix minimal:** await save metadata sesi dan workspace secara terkoordinasi; propagasi remove error kecuali NotFound; rollback/gagal tertutup bila salah satu write gagal.
- **Regression proof:** pilih dan clear workspace pada sesi berisi pesan → restart tanpa aksi tambahan; state harus sama. Inject remove/write error; UI tidak boleh menampilkan sukses permanen.

### F09 — Sedang: settings write tidak berurutan dan rollback optimistic bisa menimpa update baru

- **Lokasi:** `src/hooks/useAgent.ts:1018-1030,1053-1086`; `agent/storage.rs:84-90,152-185,910-934`.
- **Trigger/call path:** dua updateSettings A/B berdekatan, misalnya favorite atau hasil fetch-model plus save modal. Set refs optimistic, kedua `save_settings` berjalan bebas; atomic rename menentukan pemenang berdasarkan completion. Jika A gagal sesudah B sukses, catch A mengembalikan `previous` lama. Pada first-run load_settings NotFound juga bisa menulis defaults bersamaan dengan save custom.
- **Dampak:** kehilangan favorite/provider/model/setting baru atau disk/UI berbeda walaupun satu update berhasil. Atomic temp filename unik bukan ordering.
- **Fix minimal:** single settings queue + revision; rollback hanya bila revision optimistic masih milik operasi gagal. Inisialisasi defaults menggunakan create-if-absent terkoordinasi.
- **Regression proof:** deferred IPC/filesystem membalik urutan completion, termasuk gagal A setelah sukses B; readback settings dan UI wajib versi terbaru, bukan last-completing write.

### F10 — Sedang: daftar sesi/artifacts menyamarkan error sebagai daftar kosong dan save dapat menimpa file invalid

- **Lokasi:** `agent/storage.rs:207-227,248-275,424-456`; `src-tauri/src/lib.rs:113-116,138-141`; `src/hooks/useAgent.ts:343-366`.
- **Trigger/call path:** read_dir ditolak, entry gagal dibaca, JSON rusak, schema tidak cocok → list melewati semuanya/return []; wrapper mengembalikan Ok. Save snapshot kemudian `if let Ok(previous)` melewati semua kegagalan load, bukan hanya NotFound, lalu mengganti file.
- **Dampak:** pengguna melihat “tidak ada sesi” tanpa diagnosis; startup mengosongkan workspace. Snapshot baru/stale dapat menghapus satu-satunya bukti file korup yang seharusnya dapat dipulihkan. Berbeda dengan load_settings yang secara eksplisit menjaga JSON/schema invalid.
- **Fix minimal:** list Result + diagnostics per entry, jangan perlakukan PermissionDenied sebagai empty; save hanya create pada NotFound, return error/back up ketika parse/read gagal.
- **Regression proof:** fixture folder denied, satu valid + satu corrupt, schema invalid dan write setelah corrupt; error harus terlihat, sesi valid tetap ada, file invalid tidak di-overwrite diam-diam.

### F11 — Sedang: kegagalan update indeks disembunyikan; delete tidak menjamin hasil pencarian menghilang

- **Lokasi:** `agent/storage.rs:471-474,532-541`; `agent/search.rs:24-52,66-79,134-152`; `src/hooks/useAgent.ts:1220-1225`.
- **Trigger/call path:** session JSON berhasil disimpan/dihapus, database busy/corrupt/read-only membuat index_session/delete_session_index gagal → `let _ = ...` → IPC tetap sukses. Tidak terlihat startup rebuild dari existing JSON pada jalur yang diperiksa.
- **Dampak:** search stale/hilang; setelah delete, isi percakapan yang seharusnya dihapus dapat tetap muncul dari SQLite. Error pencarian frontend sendiri dikonversi ke []/console sehingga pengguna tidak tahu perlu repair.
- **Fix minimal:** nyatakan status indeks yang degraded, retry/reconcile/tombstone dan validasi hasil terhadap sesi hidup; rebuild dapat dijalankan tanpa menimpa sumber JSON. Untuk penghapusan sensitif, jelaskan residual SQLite/free-pages serta attachments yang tidak di-GC oleh delete.
- **Regression proof:** inject failure database pada save/delete lalu pulihkan; search tidak boleh memunculkan session terhapus dan reconcile harus menemukan sesi yang gagal diindeks.

### F12 — Sedang: API key/header/MCP env disimpan plaintext dan provider remote HTTP tetap diperbolehkan

- **Lokasi:** `agent/types.rs:54-71,93-99`; `agent/storage.rs:13-26,152-184,910-932`; `src-tauri/src/lib.rs:103-110`; `src/components/SettingsModal.tsx:43-45,362,768`.
- **Trigger/call path:** simpan setting provider/MCP → serde menulis seluruh apiKey/extraHeaders/env ke `~/.aether-agent/config.json`; get_settings mengembalikan struktur lengkap ke renderer. URL validasi menerima http untuk host mana pun, bukan hanya loopback Ollama.
- **Dampak:** backup/sync atau proses yang mempunyai akses user dapat membaca secret at rest; tidak ada keyring/DPAPI atau ACL eksplisit pada jalur write. HTTP remote dapat mengirim token tanpa TLS bila provider menggunakan setting itu. **Tidak menyatakan ACL aktual lemah**, tidak membaca nilai secret atau memeriksa direktori user. JSON session tidak otomatis memasukkan ProviderConfig, sehingga tidak ada bukti semua API key ikut ekspor sesi.
- **Fix minimal:** OS credential store + reference ID, renderer menerima masked metadata; batasi plaintext compatibility dengan opt-in yang jelas dan ACL privat. Tolak remote HTTP secara default, izinkan loopback local secara eksplisit. Redaksi tool result/export tetap diperlukan bila secret muncul dalam transcript.
- **Regression proof:** sentinel credential non-rahasia tidak ada di config/export/error; readback UI cukup untuk edit tanpa membocorkan existing key; remote HTTP ditolak, localhost/loopback yang disetujui tetap jalan.

### F13 — Sedang: skills workspace yang belum dipercaya dapat menyamar sebagai skill global tepercaya

- **Lokasi:** `agent/skills.rs:67-90,123-129,204-224`; `agent/runner.rs:777-790`; `src-tauri/src/lib.rs:394-405`.
- **Trigger/call path:** buka repository berisi `.aether/skills/systematic-debugging/SKILL.md` dengan nama sama seperti global → root workspace diproses dahulu → seen_names mengabaikan global → metadata mentah dimasukkan ke system prompt sebagai “trusted local”. Name/description YAML tidak ditolak jika multiline/instruksi.
- **Dampak:** konten repository mendapat framing tepercaya dan shadowing identity skill global tanpa konfirmasi. Ini jalur prompt-injection/trust confusion, bukan bukti arbitrary-code execution langsung; menjadi lebih berisiko karena F01.
- **Fix minimal:** trust approval per workspace/root, provenance yang jelas; nama ditampilkan sebagai namespace source, jangan diam-diam mengalahkan global trusted; bungkus/escape metadata sebagai data dan batasi panjang/line break.
- **Regression proof:** repo fixture punya namesake dan deskripsi multiline berisi instruksi; sebelum trust tidak masuk trusted catalog dan global tidak hilang; sesudah trust provenance tetap terlihat.

### F14 — Sedang: batas/path skills dan attachment berbeda dari primary workspace resolver

- **Lokasi:** `agent/skills.rs:141-150,240-244,294-325`; `agent/storage.rs:1031-1061`; `agent/tools.rs:1510-1557`.
- **Trigger/call path:** workspace `.aether-agent/attachments` adalah junction/symlink ke luar → save_attachment menyusun path dan atomic_write tanpa canonical containment. save_skill_markdown juga mempercayai ancestor direktori global setelah hanya memvalidasi slug. Discovery memeriksa 256 KiB, tetapi activation `read_skill_content` membaca ulang seluruh file tanpa size/regular-file check.
- **Dampak:** attachment dapat ditulis di luar workspace melalui filesystem-controlled ancestor. Skill yang ditukar/diperbesar setelah discovery dapat menyebabkan unbounded read atau mengganti konten yang dipercaya; global skill symlink parent juga redirect write. Serangan membutuhkan kontrol filesystem/root terkait; primary resolver tidak menutup alternate path ini.
- **Fix minimal:** canonicalize dan validate target parent terhadap root yang dimaksud, tolak reparse/symlink ancestor bila kebijakannya confined, gunakan handle/no-follow untuk hard guarantee. Activation baca maksimal limit+1 dan validasi regular file/identitas. Jangan menyatakan validate-then-open bebas TOCTOU.
- **Regression proof:** attachment junction keluar workspace, global skill-dir symlink, grow/swap skill antara discover/activate; harus ditolak atau kebijakan external root diakui eksplisit, penggunaan memori tetap bounded.

### F15 — Sedang: backend mutasi/ekspor tidak menggunakan registry run; abort bukan selesai

- **Lokasi:** `src-tauri/src/lib.rs:123-135,151-183`; `agent/runner.rs:673-709,712-744`; `agent/storage.rs:637-646`; `src/hooks/useAgent.ts:999-1015,1146-1168,1437-1498`.
- **Trigger/call path:** clear/edit/reset mengirim abort fire-and-forget lalu menghapus currentRunRef; abort hanya cancel token, entry active tetap ada sampai finish. send baru bisa sampai sebelum cleanup selesai dan ditolak “already active”. Export memeriksa active ref sebelum dialog save tetapi tidak mengunci operasi atau memeriksanya lagi sesudah dialog; run dapat dimulai ketika dialog menunggu. Native export/delete/save/compact tidak memeriksa registry; compact hanya melihat flag snapshot isStreaming.
- **Dampak:** run baru ditandai gagal setelah UI lebih dulu mengubah riwayat; ekspor dapat berisi snapshot run yang sedang aktif walaupun guard UI menjanjikan stop dulu. Clear/delete tidak berarti proses lama berhenti menyentuh workspace atau reflection terlepas selesai. Reflection memakai CancellationToken baru dan registry terpisah (`runner.rs:1599-1635`).
- **Fix minimal:** abort-and-await-finish/lease state native, guard per sesi untuk mutasi destruktif/export/review dan transisi run. Pemeriksaan dan registrasi review/run harus atomik; jangan memakai flag pesan sebagai sumber kebenaran aktivitas.
- **Regression proof:** abort cleanup ditahan lalu retry; hasil harus waiting/cancelled, bukan merusak turn dan failed start. Mulai run saat dialog ekspor terbuka; export ditolak/ditunda konsisten. Delete/clear saat delayed reflection tidak boleh menulis skill tanpa kebijakan yang jelas.

### F16 — Sedang: export dapat menimpa state internal atau keluar workspace tanpa konfirmasi jika destination disuplai

- **Lokasi:** `src/components/ChatCanvas.tsx:1131-1138`; `src/hooks/useAgent.ts:1154-1168`; `src-tauri/src/lib.rs:156-177`; `agent/storage.rs:701-717,910-934`.
- **Trigger/call path:** `/export ../outside.md` atau `/save <absolute-path>` → requestedDestination melewati native save dialog → relative path hanya di-join dengan cwd, absolute diterima mentah → atomic replace. Tidak ada check destination sama dengan config/session/search/state atau overwrite confirmation.
- **Dampak:** ekspor yang sengaja diarahkan ke luar workspace adalah kemampuan lokal yang mungkin diinginkan, **bukan otomatis sandbox escape**. Bug konkret: tabrakan target dapat menghancurkan config/session (Markdown ke session JSON), dan relative traversal mengejutkan jika UI menganggap workspace-contained.
- **Fix minimal:** pisahkan trusted native-dialog export dari quick-path export; quick-path resolve confined atau konfirmasi absolute/outside/overwrite. Larang overwrite state internal dan sumber session yang sedang diekspor; validasi format/ekstensi.
- **Regression proof:** export ke path sesi sendiri, config.json, ../outside dan existing file; tidak boleh silent overwrite. Absolute hasil native picker yang disetujui tetap didukung.

### F17 — Rendah: parity schema cukup luas, tetapi notice hilang dan optional TypeScript bukan null Rust

- **Lokasi:** `src/types/agent.ts:13-26,67-87,139-183,262-274`; `agent/types.rs:196-220,277-316,343-419,576-603`.
- **Trigger/call path:** frontend mengirim ChatMessage.notice → Rust ChatMessage tidak punya field dan Serde tidak deny_unknown_fields → notice hilang saat save/load/export. Field Option lain tanpa skip_serializing_if menghasilkan null (mis. tool.result/artifactId, council.reasoning, session.workspaceCwd, maxTokens), sementara TS kebanyakan memakai `?: T` yang tidak mendeklarasikan null.
- **Dampak:** model tipe frontend bukan wire contract penuh; konsumen yang hanya mengecek undefined bisa keliru. Notice bisa memang transient dan sekarang dihapus saat done, maka kehilangan notice bukan bukti kehilangan field durable utama. Preamble, segments, attachments, councilTurn/chosenWinner, modelOverride, run identity/status dan selfImprovement **sudah ada di Rust**—jangan laporkan sebagai field hilang.
- **Fix minimal:** tandai notice sebagai UI-only di tipe terpisah atau persist eksplisit; generate TS dari Rust/JSON schema dan tentukan nullability. Runtime enum string Rust (`role/status/type/chosenWinner`) dan ToolCallItem.args Value juga lebih longgar dari union/Record TS.
- **Regression proof:** fixture seluruh field non-default frontend → native roundtrip → strict runtime schema; nullable/omitted punya keputusan eksplisit, unknown durable fields tidak terbuang tanpa diagnostic.

### F18 — Rendah: CSP nonaktif memperluas dampak kompromi renderer

- **Lokasi:** `src-tauri/tauri.conf.json:25-27`; `capabilities/default.json:5-11`; `src-tauri/src/lib.rs:861-905,501-510,103-110`.
- **Trigger/call path:** apabila ada injeksi script renderer pada komponen lain atau dependency, tidak ada CSP untuk menahan script/network. Renderer main memiliki akses custom command termasuk shell execution dan get_settings. Capability plugin bukan workspace policy bagi implementasi custom Rust.
- **Dampak:** defense-in-depth hilang pada renderer berprivilege tinggi. MarkdownRenderer menggunakan ReactMarkdown dan memblokir beberapa scheme; **tidak ditemukan/dibuktikan XSS dari raw Markdown pada scope ini**, dan `shell:allow-execute` tanpa scope tidak boleh disamakan dengan terbukti unrestricted plugin execution.
- **Fix minimal:** CSP script-src self dengan resource origin minimal dan IPC origins yang dibutuhkan Tauri, capability least privilege, gate native pada command sensitif. Jangan aktifkan remote IPC hanya untuk menyelesaikan masalah UI.
- **Regression proof:** inline/injected script dan unauthorized external connect diblokir; aplikasi normal, dialog, provider melalui Rust, dan asset rendering tetap berfungsi di build terpaket.

## Catatan tambahan: locks, durability, validasi, dan skala

1. **Tidak ada bukti deadlock session mutex reentrant:** public storage mengambil SESSION_IO_LOCK lalu memakai helper unlocked. Mutex workspace std dilepas setelah clone sebelum await. Ini perlu dipertahankan; jangan “memperbaiki” dengan menghapus lock yang memang sudah melindungi read-modify-write native.
2. **Head-of-line blocking:** `list_sessions`/`list_all_artifacts` memegang satu global mutex sambil membaca semua file penuh; export juga menahannya selama menulis destination, dan save menjalankan SQLite sinkron sebelum melepasnya (`storage.rs:207-275,471-474,701-717`). Satu sesi besar/destination lambat menunda semua sesi. `merge_self_improvements` mencari seluruh pesan sebelumnya untuk setiap pesan baru (`372-384`), sehingga cost pencocokan kuadratik. Minimal: snapshot read di bawah per-session lock, export write di luar lock, index spawn_blocking/queue, map by (id,runId), limit ukuran dokumen.
3. **Atomicity bukan power-loss/multi-process guarantee:** atomic_write memakai temp unik, write_all, sync_all, rename dan cleanup error; itu menghindari target setengah jadi pada kegagalan biasa. Parent directory tidak di-sync, tidak ada transaction antara JSON/index/workspace, tidak ada lock lintas proses. Jangan klaim durability penuh hanya karena sync_all file temp.
4. **Validation gaps settings:** save memvalidasi URL/safe provider ID/duplicate provider/temperature/maxTurns/maxTokens=0, tetapi tidak activeProviderId exists, duplicate MCP IDs, model nonempty, atau threshold token yang masuk akal (`storage.rs:152-185,981-986`). load hanya memperbaiki active provider, empty providers, maxTurns dan temperature nonfinite; temperature finite out-of-range dan maxTokens=0 dari config buatan manual tetap bisa diteruskan. Gunakan satu validate/normalize untuk load/save/run (dengan pesan migrasi, bukan diam-diam mengubah pengguna).
5. **Skill overwrite contract:** `save_skill` IPC memaksa overwrite=true (`lib.rs:409-415`); tool learn_skill default false tetapi menggunakan exists-check lalu atomic replace (`skills.rs:297-325`). Dua create bersamaan dengan overwrite=false dapat sama-sama lolos lalu saling menimpa. Gunakan exclusive create/atomic no-clobber dan expose overwrite eksplisit pada IPC. Name, YAML serialization dan batas description/body sudah divalidasi; jangan melaporkan traversal slug sederhana yang sebenarnya ditolak.
6. **Read-only command sebenarnya menulis:** `list_skills` → load_skills → init_skills_environment → ensure_starter_skills membuat folder/starter global dan menelan error (`skills.rs:27-65,81-82`). Ini perilaku implementasi, bukan audit ini memanggilnya. Pisahkan bootstrap eksplisit dari discovery; jangan silently reseed skill yang sengaja dihapus operator.
7. **File identity legacy:** parser/list tidak memverifikasi nama file sama dengan data.session.id (`storage.rs:214-225,285-305`); JSON hasil restore/import manual dengan ID salah dapat muncul di sidebar tetapi load/delete menyasar file lain. Validasi identitas sumber dan quarantine mismatch.
8. **Session metadata:** substantive-change heuristic membandingkan jumlah pesan/artifacts dan pesan terakhir, bukan seluruh content/modelOverride/artifact/reasoning (`storage.rs:438-455`). Edit non-last/model-only bisa tersimpan tanpa updatedAt baru. Return togglePin meng-clone sebelum save memperbarui metadata (`527-529`), meski refreshSessions mengoreksi sidebar. Gunakan revision dan metadata readback konsisten.
9. **Attachment retention:** delete/clear hanya menghapus sesi/isi, bukan file attachment pada workspace atau config_dir (`storage.rs:532-541,1031-1061`; `useAgent.ts:1006-1015`). Ini perlu kebijakan retensi/reference counting; bukan klaim semua attachment aman dihapus karena bisa dipakai sesi lain.
10. **Startup loading race:** startup memanggil load_session tetapi tidak menunggu sebelum `setIsLoading(false)` (`useAgent.ts:349-369`). User dapat send ke sesi baru ketika load lama masih pending, lalu response load mengganti data karena navigation counter tidak bertambah saat send. Defer publish-ready sampai restore selesai atau gunakan epoch mencakup send; regression dengan delayed load dan input segera.
11. **Clear buffer race:** clearCurrentSession tidak flush/reset text/council/terminal buffer ataupun frameRef (`useAgent.ts:999-1015`); callback frame lama atau buffer dapat tercampur ke turn berikutnya jika clear lalu send sebelum flush. Reset semua buffer atomik dengan epoch/session/run identity.

## Regression plan yang paling bernilai

Tidak ada test yang dijalankan pada tahap ini. Bukti tests existing yang dibaca: storage unit tests pada `storage.rs:1077-1375`, schema tests `types.rs:605-722`, skills tests `skills.rs:335-497`, reflection hook tests `src/hooks/useAgent.reflection.test.tsx:54-164`. Tests tersebut memberi fixture berguna, bukan bukti pass pada checkout ini.

Urutan implementasi/perbaikan yang disarankan:

1. Native approval gate dan launcher tanpa cmd; gunakan fixture/model stub serta process stub, bukan credential/provider nyata.
2. Workspace transition transaction + request workspace identity; delayed load + unavailable workspace + immediate send.
3. Durable run checkpoint dan run mutation lease; restart sebelum done, cancel cleanup, delete/clear/reflection overlap.
4. Revision/CAS untuk sesi/settings, tombstone delete, stale unpin; dua writer dan reorder/failure injection.
5. Compact dua kali + full canonical replay/export preamble/segments/council/tools; roundtrip wire schema/nullability.
6. Denied/corrupt storage dan unavailable search index; error diagnostic dan recovery tidak menimpa sumber.
7. Symlink/junction alternate paths, bounded skill activation, untrusted workspace namesake; uji pada Windows.
8. Packaged CSP/capability smoke serta remote-HTTP/keychain migration menggunakan sentinel, tanpa secret aktual.

## Evaluasi kualitas laporan

- **Akurasi 4/5:** setiap temuan menunjuk implementasi; exploit opener Windows dan semua race belum diuji runtime sehingga diberi batas statis.
- **Kelengkapan 4/5:** seluruh scope utama dan boundary frontend dibaca; internals provider/MCP/process sengaja bukan audit mendalam lapisan ini, ACL aktual tidak diperiksa.
- **Kejelasan 4/5:** severity, trigger, dampak, fix dan proof tersedia; beberapa catatan tambahan masih perlu dipecah menjadi ticket kecil saat remediasi.
- **Actionability 4/5:** setiap temuan prioritas memiliki regression scenario; belum ada patch/test karena mandat read-only.
- **Keringkasan 4/5:** laporan lapisan ini panjang untuk menjaga evidentiary detail; ringkasan audit parent sebaiknya memprioritaskan F01–F04 dan data-loss F02/F05/F06.
- **Rata-rata 4,0/5.** Perbaikan bukti paling penting: uji launcher Windows secara terisolasi; race injection untuk workspace/revisions/checkpoint; parity/export fixture end-to-end. Operator wajar setuju bahwa hasil ini review statis, bukan verifikasi runtime atau kesiapan rilis.
