# Audit lifecycle runtime agent — review read-only

## Ringkasan dan batas bukti

Audit source lengkap dilakukan atas lima file yang ditugaskan. Semua temuan di bawah berstatus **confirmed secara statis**, kecuali bagian residual yang diberi label uncertain. Tidak ada klaim reproduksi runtime. Build, test, lint, provider request, spawning proses aplikasi, dan perubahan source **tidak dilakukan**, sesuai pembagian gates milik parent. Satu-satunya write adalah laporan ini.

Baseline `git status --short` awal dan pembacaan ulang sebelum laporan: hanya `?? docs/audit-evidence-tools-2026-11-08.md`. File tersebut tidak dibaca, disentuh, atau diubah. Tidak membuka credential/config pengguna. Tidak ditemukan `AGENTS.md` pada `D:/`, root repo, `src-tauri/`, `src-tauri/src/`, atau `src-tauri/src/agent/` melalui pemeriksaan path eksplisit.

Beberapa `read_file` timeout dan satu pencarian gagal dengan `No shell found`. Pembacaan yang gagal diselesaikan melalui Python read-only (`Path.read_text`) di `execute_code`; tidak ada data source yang ditebak. Perintah pemeriksaan AGENTS mencetak seluruh hasil tetapi helper terminal timeout setelahnya; hasil keberadaan file juga konsisten dengan pencarian repo. Tidak ada gates yang dicoba diam-diam setelah kegagalan tool.

Severity: **C** kritis, **H** tinggi, **M** sedang, **L** rendah. Tidak ada temuan C yang dapat dibuktikan dari scope ini. Hitungan heading diverifikasi programmatically: **17 temuan confirmed-statis (6 H, 11 M)**; lima residual dipisah dan tidak dihitung sebagai confirmed findings.

Pemeriksaan git akhir sesudah write laporan menampilkan perubahan concurrent `M package.json` dan `M vitest.config.ts`, selain untracked evidence awal dan `docs/audit/`. Kedua file tersebut **tidak dimodifikasi subagent ini**; perubahan parent/workstream lain dipertahankan tanpa inspeksi ulang atau overwrite. Tidak ada build/test yang dijalankan oleh reviewer ini.

## Coverage

| File scope | Coverage source | Area yang ditelusuri |
|---|---|---|
| `src-tauri/src/agent/runner.rs` | seluruh 1–2096 | emitter; skills; subagent loop/retry/synthesis; begin/abort/finish; normal/council runs; parallel dispatch; auto/manual reflection; history replay; micro-compaction; seluruh tests |
| `src-tauri/src/agent/reflection.rs` | seluruh 1–727 | parsing/validation; deadline/cancel; redaction/context bounds; existing skills; snapshot checks; writer mutex/atomic commit; symlink/reparse validation |
| `src-tauri/src/agent/reflection_tests.rs` | seluruh 1–1024 | seluruh fixtures dan assertions, termasuk mock HTTP, cancellation, concurrency, junction, unmanaged/manual skills |
| `src-tauri/src/agent/tasks.rs` | seluruh 1–439 | spawn/stdin/output monitor; task registry/status/list/kill; seluruh tests |
| `src-tauri/src/agent/types.rs` | seluruh 1–722 | semua serde/default kontrak settings/request/history/event/artifact/subagent; seluruh tests |

Total coverage utama: **5008 baris**. File terkait dibaca pada call path relevan, bukan diaudit seluruhnya: `src-tauri/src/lib.rs`, `agent/tools.rs`, `agent/provider.rs`, `agent/storage.rs`, `agent/mcp.rs`, `agent/skills.rs`, `src/hooks/useAgent.ts`, `src/types/agent.ts`, `src/components/MessageRow.tsx`, serta `Cargo.toml` dan `package.json`. Manifest menunjukkan Rust/Tauri 2 dan React 19; versi terpasang tidak diverifikasi.

## Temuan confirmed — prioritas perbaikan

### RT-01 — H — Request Review tidak memiliki enforcement native; subagent bahkan tidak mewarisi mode

**Lokasi:** `src-tauri/src/agent/runner.rs:800–806,1122–1126,1190–1218,1219–1220`; `runner.rs:237–250,294–295,330–334`; `src-tauri/src/agent/types.rs:110–122,417–418`; terkait `agent/tools.rs:37–40,290–319`.

**Trigger / call path:** operator memilih `RequestReview` → `send_message` → `run` menambahkan instruksi bahasa alami → provider mengeluarkan `write_file`, `run_command`, `manage_task.kill`, atau `spawn_subagent` → dispatcher langsung menjalankan. `ToolContext` hanya membawa cwd/cancel/sink, tanpa mode atau approval. Pencarian seluruh Rust menemukan konsumsi `execution_mode` hanya untuk string prompt dan definisi types. `SubagentParams` tidak membawa execution mode; prompt subagent tidak mengandung larangan mode parent.

**Dampak:** UI/mode menjanjikan persetujuan eksplisit sebelum execute/write tetapi tidak ada gate yang dapat menahannya. Model yang tidak patuh atau hasil prompt injection dapat menulis/menjalankan tanpa persetujuan; delegasi menghilangkan bahkan guard berbasis prompt. Default missing mode adalah Turbo, bukan fail-closed; ini memperbesar dampak request lama yang tidak menyertakan field.

**Fix minimal:** bawa policy eksplisit pada ToolContext/SubagentParams; enforce sebelum setiap dispatch native/MCP/subagent. Untuk patch aman paling kecil, reject execute/write di RequestReview sampai ada approval native dengan run/tool/args binding; jangan mengandalkan model meminta izin di teks. Mode child tidak boleh lebih permisif dari parent. Perubahan default perlu keputusan kompatibilitas tersendiri, bukan diselipkan diam-diam.

**Test pembukti:** mock provider memaksa tool write/command dalam RequestReview, termasuk melalui spawn_subagent. Assert tidak ada file/PID baru sebelum approval; approval hanya berlaku untuk tool dan args yang sama. Kirim request tanpa executionMode dan assert policy default yang disepakati.

### RT-02 — H — `send_input` dapat mengunci task sehingga kill/cancel tidak pernah memperoleh lock

**Lokasi:** `src-tauri/src/agent/tasks.rs:118–130,224–228,325–338,349–350`; terkait `agent/tools.rs:314,1280–1285`; `runner.rs:1283`.

**Trigger / call path:** task tidak membaca stdin → writer terblokir di `child_stdin.write_all` → antrean stdin kapasitas 32 terisi → `send_input` selanjutnya menunggu `tx.send(...).await` sambil memegang **read guard TaskEntry** → `kill_task` membutuhkan write guard yang tidak bisa didapat. Monitor juga membutuhkan write guard saat output/status diperbarui. `manage_task` tidak menerima cancellation token; parent `join_all` harus menunggu call tersebut selesai meski abort diminta.

**Dampak:** satu task yang macet dapat menjadi tidak bisa dibunuh dari UI maupun agent; run macet di tool dispatch, Stop tidak menyelesaikan done. Tidak ada bukti deadlock std::sync::Mutex pada registry runner; deadlock konkret adalah async RwLock di sini.

**Fix minimal:** clone sender dan status di dalam blok lock pendek lalu drop guard **sebelum** send await. Select send dengan cancel dan timeout; beri batas input bytes. Writer stdin juga harus select cancel. Berikan acknowledgement setelah write/flush bila pesan ingin disebut delivered, bukan sekadar enqueued.

**Test pembukti:** fixture child menahan stdin tanpa membacanya; isi pipe dan channel dengan input berukuran terkontrol, mulai send berikutnya, kemudian kill. Gunakan timeout ketat untuk memastikan kill, parent abort, dan monitor completion tetap selesai; jangan hanya assert field status.

### RT-03 — H — Output background tidak dibatasi bytes; pembatas 500 baris tidak mencegah OOM/disk exhaustion

**Lokasi:** `src-tauri/src/agent/tasks.rs:13,175–176,187–188,219–228,261–270,292–299`.

**Trigger / call path:** background command mengeluarkan data panjang tanpa newline → `BufReader::lines().next_line()` mengalokasikan seluruh baris sebelum channel bounded menerima apa pun. Alternatif: 500 baris masing-masing sangat besar tersimpan utuh; `list_tasks` menyalin dan join seluruh buffer untuk semua task. Log append terus berlangsung tanpa quota/rotasi.

**Dampak:** memori dapat tumbuh sampai crash meskipun mpsc dibatasi 128 dan VecDeque dibatasi 500 elemen. UI polling dapat menggandakan alokasi besar; stdout berkecepatan tinggi memenuhi disk pengguna. Invalid UTF-8 juga menghentikan reader secara diam-diam karena `while let Ok(...)`, bukan error task yang terlihat.

**Fix minimal:** reader chunk byte terbatas seperti foreground command, lossy UTF-8 yang terkontrol, byte budget per baris dan per task; ring buffer berdasarkan bytes; batas ukuran log + rotation/truncation marker. Jangan mengembalikan seluruh recent_output setiap list bila status ringkas cukup.

**Test pembukti:** fixture mengeluarkan flood tanpa newline dan baris raksasa melewati byte cap; assert ukuran buffer/log/respons IPC tetap bounded dan kill tetap responsif. Tambahkan byte non-UTF8 dan assert output/status error tidak hilang tanpa penjelasan.

### RT-04 — H — Replay mengekspor pending/cancelled/phantom tool calls tanpa pasangan hasil

**Lokasi:** `src-tauri/src/agent/runner.rs:1767–1786`; pembentuk pending `runner.rs:892–916`; terkait `src/hooks/useAgent.ts:49–64,427–460,767`.

**Trigger / call path:** provider baru mengirim sebagian tool delta, kemudian operator Stop atau stream gagal → UI menyimpan ToolCallItem pending/cancelled dengan `result: None` → prompt berikutnya memasukkan seluruh message lama → `append_history_message` mengekspor **semua** tools sebagai assistant.tool_calls, tetapi hanya membuat role=tool bila result Some. Hal yang sama berlaku untuk tool phantom dari retry RT-10.

**Dampak:** payload OpenAI mempunyai tool call tanpa tool response sebelum user berikutnya; provider strict menolak history dan sesi tidak dapat dilanjutkan normal. Anthropic conversion juga menghasilkan tool_use yang tidak diselesaikan. Raw args parsial dapat diekspor sebagai `{}`/object sementara, bukan invocation yang pernah dijalankan.

**Fix minimal:** replay hanya pasangan invocation/result yang valid; jangan mengekspor tool streaming yang tidak pernah dieksekusi. Untuk invocation yang memang dimulai tetapi dibatalkan, simpan terminal result cancellation eksplisit dan pasangan tool response. Normalisasi history sekali sebelum konversi provider, bukan hanya mengganti status visual.

**Test pembukti:** capture request berikutnya sesudah abort di tengah args, sesudah provider failure, dan sesudah cancel saat tools berjalan. Assert setiap exported call id memiliki tepat satu response sebelum turn berikutnya, dan tool yang hanya berupa delta tidak pernah direplay sebagai executed call.

### RT-05 — H — `join_all` menjalankan mutation satu resource bersamaan; snapshot check tidak atomic dengan commit

**Lokasi:** `src-tauri/src/agent/runner.rs:1067–1077,1219–1220,1283`; terkait `agent/tools.rs:508–536,539–547,577–643`, `agent/storage.rs:910–934`.

**Trigger / call path:** satu provider turn meminta dua `edit_file` pada file yang sama → semua PreparedCall dijalankan bersamaan → kedua tool membaca original yang sama → kedua `guarded_atomic_write` membandingkan file lama sebelum salah satunya rename → masing-masing atomic_write menulis temp dan rename → commit terakhir menimpa commit sebelumnya. Ini schedule yang diizinkan oleh semua await, bukan dugaan based on grep. `read_file` + `write_file` atau perintah dengan dependency dalam batch juga tidak memiliki ordering.

**Dampak:** lost update dengan dua tool sama-sama mengaku sukses; subagents paralel yang menulis file sama mempunyai risiko serupa. Atomic rename hanya mencegah torn write, bukan serialisasi compare→write. Test `concurrent_tool_calls_execute_simultaneously` di runner:2084–2094 hanya menguji `join_all` atas sleep, tidak menguji kebijakan dispatch atau integritas file.

**Fix minimal:** jangan paralelkan semua write/exec tanpa klasifikasi. Serialisasikan tool mutating dalam satu run sebagai langkah cepat; untuk perlindungan lintas run/subagent gunakan resource lock yang mencakup read snapshot→compare→rename dalam helper write/edit. Reads/subagents independen tetap boleh paralel. Perbaikan runner saja tidak menyelesaikan writer eksternal.

**Test pembukti:** barrier test memaksa dua edit membaca versi lama dan melewati compare bersamaan; hasil akhir harus memuat kedua perubahan yang kompatibel atau salah satunya menerima conflict, tidak dua success dengan satu perubahan hilang. Test juga dependency write→read pada satu batch.

### RT-06 — H — Background task global tidak mempunyai owner; semua session/subagent dapat mengelola proses lain

**Lokasi:** `src-tauri/src/agent/tasks.rs:17–35,43–46,66–72,292–369`; `runner.rs:330–334,1122–1126`; terkait `agent/tools.rs:977–979,1237–1290`, `src-tauri/src/lib.rs:814–826`.

**Trigger / call path:** session A menjalankan daemon pada workspace A → session B atau subagent workspace B memanggil `manage_task list` → global TaskManager mengembalikan command/log semua tasks → taskId A dapat digunakan untuk status/send_input/kill tanpa pengecekan owner. TaskInfo/TaskEntry tidak menyimpan runId, sessionId, cwd, atau subagentId. Semua entry IPC background juga tanpa scope.

**Dampak:** isolasi konteks antar pekerjaan hilang: command/log rahasia project A dapat masuk provider sesi B; agent B dapat mengubah stdin atau menghentikan server A. Parent abort tidak membatalkan token daemon yang dibuat baru di tasks:135. Persistensi daemon setelah run **sendiri bukan bug** karena isDaemon memang background; bug adalah tidak ada ownership/policy yang membedakan deliberate detached service dari child milik run, apalagi dari subagent.

**Fix minimal:** tambahkan TaskOwner (session/run/subagent/workspace), masukkan lewat ToolContext, filter dan authorize list/status/input/kill untuk caller agent. Bila UI global task panel memang diinginkan, pisahkan API operator yang eksplisit dari API agent ter-scope. Tetapkan policy detach/cancel, jangan diam-diam membunuh semua daemon pada setiap Done.

**Test pembukti:** dua sessions/workspaces dan dua subagents; caller B tidak melihat/mengelola task A melalui agent API. Operator global tetap dapat mengelola bila diberi akses eksplisit. Assert child auto-cancel atau survive hanya sesuai policy detach yang dipilih.

### RT-07 — M — Task monitor menunggu `child.wait()` di dalam branch tanpa cancellation

**Lokasi:** `src-tauri/src/agent/tasks.rs:208–258`, khususnya `230–242`; `tasks.rs:212–215`.

**Trigger / call path:** child menutup stdout+stderr tetapi tetap hidup → kedua reader selesai → line_rx.recv() mengembalikan None → monitor masuk `child.wait().await` di dalam branch select. Setelah masuk branch, select luar tidak lagi dipoll sehingga monitor_cancel tidak dapat menginterupsi wait. Pada Unix `kill_task` hanya cancel token (Windows taskkill dapat menutupi bug bila sukses); pada Windows bug tetap muncul bila taskkill gagal/hang, karena hasilnya diabaikan.

**Dampak:** task dapat ditandai killed tetapi proses dan monitor tetap hidup; completed_at tidak terisi. Setelah shell exit namun descendant menahan pipe, kebalikannya terjadi: process_exited true tetapi monitor menunggu EOF tanpa batas dan status tetap running.

**Fix minimal:** jadikan channel-closed sebagai flag, disable cabang recv, tetap polling child.wait dan cancel di select luar. Sesudah process exit, drain dengan deadline lalu stop readers. Simpan JoinHandle reader/stdin untuk teardown deterministik.

**Test pembukti:** fixture menutup kedua stream lalu sleep; kill harus mengakhiri PID dan mengisi completed_at di semua platform. Fixture shell exit dengan grandchild memegang pipe harus tidak menahan status running selamanya.

### RT-08 — M — Kill mengeklaim sukses sebelum exit; PID shell yang sudah exit dapat didaur ulang

**Lokasi:** `src-tauri/src/agent/tasks.rs:247–255,273–285,349–368`.

**Trigger / call path:** shell task exit tetapi descendant masih memegang stdout/stderr → monitor menyimpan final_exit_code namun `info.status` dan `info.pid` tetap running/PID lama sampai stream EOF. Operator kill → taskkill /PID PID-lama /T /F. Jika PID sudah reuse oleh OS, target dapat menjadi proses lain. Jalur biasa juga menandai killed sebelum hasil taskkill diketahui, menelan error, lalu return terminated tanpa menunggu monitor/child exit. Pada Unix tidak ada process-group/tree kill sama sekali.

**Dampak:** false-positive termination, descendant tersisa, dan pada schedule PID reuse dapat menargetkan proses yang tidak dimiliki task. Kondisi PID reuse adalah conditional OS schedule; penggunaan PID stale dan return sukses tanpa verifikasi confirmed di source.

**Fix minimal:** gunakan owned process group/Windows Job Object sejak spawn; kirim kill ke monitor yang memegang Child/process-group handle, bukan std::process::Command blocking dari caller. Gunakan state stopping→killed hanya setelah wait/verification; propagasikan error. Bila belum memakai job, minimal clear PID/update exit state segera saat wait selesai dan jangan pernah taskkill PID yang sudah direap.

**Test pembukti:** shell exit dengan descendant hidup, kegagalan kill yang diinjeksi, repeated kill, serta process tree tiga tingkat. Assert kill return tidak sukses bila proses belum mati dan tidak ada request kill ke identifier yang sudah retired.

### RT-09 — M — ID task reset pada restart dan log append mencampur output dari pekerjaan berbeda

**Lokasi:** `src-tauri/src/agent/tasks.rs:49–55,59–63,73–77,197–203`.

**Trigger / call path:** app berjalan pertama kali membuat task_1.log → app restart → counter kembali 1 tetapi log lama masih ada → task_1 baru append ke file yang sama. Dua TaskManager pada tests juga memakai task_1/log path global yang sama. Log open/create-dir/write failures diabaikan; spawn tetap menyampaikan Log Path sebagai bila tersedia.

**Dampak:** log lintas sesi/restart tercampur, stale taskId dalam conversation dapat mengacu ke proses berbeda setelah restart, dan full output tidak dapat dipercaya; pada permission/disk-full output di disk tidak ada meskipun start sukses mengiklankan path.

**Fix minimal:** gunakan UUID/launch namespace pada taskId/log path dan create_new; kegagalan menyiapkan log dipropagasikan atau diekspos sebagai log_error. Pisahkan test log roots dari home pengguna. Retain ID tidak boleh berarti restore proses tanpa verifikasi identitas OS.

**Test pembukti:** dua managers dengan root log yang sama dan restart simulasi menghasilkan ID/path berbeda tanpa kontaminasi output; inject permission/disk write failure dan assert error/status visible.

### RT-10 — M — Retry reset merusak teks turn terdahulu dan meninggalkan tool phantom/segments stale

**Lokasi:** `src-tauri/src/agent/runner.rs:875–916,930–946`; terkait `src/hooks/useAgent.ts:379–398,427–460`, `src/components/MessageRow.tsx:157–185`.

**Trigger / call path:** agent sudah menyelesaikan satu tool turn dan menampilkan intermediate text → provider turn berikutnya mengirim sebagian text/reasoning lalu timeout → notice reset_partial true → hook mengosongkan **seluruh** message.content/reasoning, bukan delta attempt ini. `preamble`, `segments`, dan toolCalls tidak di-reset; renderer utama justru menggunakan segments. Jika attempt gagal setelah hanya tool delta, flag had_chunks tetap false sehingga reset_partial false; pending tools dari attempt gagal tetap ada. UI ids memasukkan turns tetapi tidak attempt; retry yang mengganti daftar tools meninggalkan pending item pada index yang tidak muncul lagi.

**Dampak:** UI dapat menampilkan teks retry ganda/failed-attempt sebagai sah, sementara payload next-run kehilangan intermediate text yang dihapus. Phantom call menjadi pending selamanya dan ikut merusak replay (RT-04) serta membuat manual review ditolak.

**Fix minimal:** buat checkpoint per provider attempt dan rollback hanya text/reasoning/tools/segments yang berasal dari attempt tersebut; identitas event membawa turn+attempt atau native stable attempt ID. Tool delta juga dihitung sebagai emitted partial. Jaga canonical message state dan rendered segments konsisten.

**Test pembukti:** turn1 sukses dengan tools; turn2 attempt1 mengeluarkan text+dua tool deltas lalu retry, attempt2 hanya satu tool lalu final. Assert output turn1 dipertahankan, attempt1 hilang dari semua representasi, tidak ada orphan tool, dan request replay sama dengan output yang ditampilkan.

### RT-11 — M — Subagent kehilangan AGENTS.md dan operator instructions, bukan hanya mengisolasi history

**Lokasi:** `src-tauri/src/agent/runner.rs:237–250,294–304`; bandingkan parent `runner.rs:780–806` dan council `runner.rs:1378–1382`.

**Trigger / call path:** workspace mempunyai AGENTS.md atau operator memasang systemPrompt (misalnya read-only, jangan ubah file X, pakai tool tertentu) → parent spawn_subagent → child system hanya role/cwd/task dan skill catalog; tidak memanggil load_workspace_memory dan tidak membawa operator/policy context. History isolation child memang disengaja dan baik, tetapi workspace/operator constraints tidak sama dengan conversation yang harus diisolasi.

**Dampak:** task yang didelegasikan dapat menulis di luar instruksi kerja yang diterapkan parent. Ini terpisah dari approval RT-01: bahkan pada Turbo, batas read-only dari AGENTS/operator hilang.

**Fix minimal:** konstruksi shared immutable run-policy context sekali, lalu bawa operator instructions, root AGENTS.md, execution mode ke child. Jika subpath mempunyai instruksi tambahan, resolve secara bounded dan tambahkan tanpa melemahkan root policy. Jangan menyalin seluruh transcript parent atau credentials untuk memperbaikinya.

**Test pembukti:** capture request subagent dengan operator sentinel dan root/subpath AGENTS sentinel; assert constraint hadir, history private parent tidak hadir, dan child cwd tetap target yang tervalidasi.

### RT-12 — M — Tanpa workspace, subagent mengaktifkan seluruh OS temp sebagai workspace file tools

**Lokasi:** `src-tauri/src/agent/runner.rs:256–263,280,330–334`; terkait `agent/tools.rs:1515–1519`.

**Trigger / call path:** aplikasi tanpa active workspace → parent native file tools menolak dengan No active workspace → provider spawn_subagent tanpa subpath → child target_dir = std::env::temp_dir() → sub_context.cwd terisi temp → read_file/list/search/write tidak lagi melihat no-workspace dan diberi akses ke seluruh temp root. Larangan subpath ketika cwd kosong tidak menutup kasus ini.

**Dampak:** batas no-workspace berubah hanya karena delegasi; child dapat membaca/mengubah file temp milik aplikasi lain dan memasukkan isinya ke provider. Ini tidak mengklaim shell disandbox: run_command memang arbitrary execute; inconsistency yang ditemukan adalah aktivasi file-tool root tanpa pilihan operator.

**Fix minimal:** reject subagent dengan native file work bila workspace tidak dipilih, atau buat dedicated per-run sandbox temp kosong dengan ownership/lifetime eksplisit. Jangan memakai temp root bersama sebagai active workspace.

**Test pembukti:** no workspace + subagent read sentinel pada temp root harus ditolak; bila memilih sandbox, root unik tidak dapat menjangkau sibling temp. Perilaku parent/child terhadap no-workspace harus sama.

### RT-13 — M — Synthesis subagent menelan failure/cancel dan tetap melaporkan completed successfully

**Lokasi:** `src-tauri/src/agent/runner.rs:515–525,528–580,582–625`.

**Trigger / call path:** child habis max_turns atau final report kurang lengkap → needs_synthesis → synthesis provider gagal/timeout/cancel → hanya `is_ok && nonempty` yang diperiksa, error tidak dipropagasi; setelah await tidak ada check cancellation lagi. Fallback menjadi intermediate findings atau `Subagent completed exploration in N turns`, lalu artifact **Status: Completed**, progress completed, ToolExecutionResult success. Saat cancel selama synthesis, transport dapat return Ok(cancelled=true) dengan partial text; `synth_res.is_ok()` tetap true.

**Dampak:** parent diberi sukses palsu ketika final synthesis gagal; Stop dapat diikuti event completed yang menimpa status cancelled subagent di UI. Parent cancellation memang akan menghentikan run luar, tetapi tidak memperbaiki progress/report child yang salah. Reaching tool budget juga tidak dibedakan dari selesai investigasi.

**Fix minimal:** match synth_res secara penuh, cek result.cancelled dan token setelah await, return cancelled outcome sebelum emit completed. Bila synthesis wajib untuk menyelesaikan tugas dan gagal, return failure/incomplete dengan partial report yang ditandai; jangan mengarang success fallback. Simpan alasan budget exhaustion bila investigasi belum konklusif.

**Test pembukti:** cancel tepat setelah synthesis dimulai; provider synthesis 503/timeout; child terus tools sampai 12 turns. Assert tidak ada completed/success outcome untuk cancel/failure dan partial findings tetap tersedia dengan status benar.

### RT-14 — M — Artifact/tool evidence subagent dibuang sebelum mencapai parent/persistence

**Lokasi:** `src-tauri/src/agent/runner.rs:330–334,493–511`; `runner.rs:605–625`; dibanding parent `runner.rs:1114–1125,1246–1277`.

**Trigger / call path:** child `edit_file`, read panjang, atau run_command menghasilkan ToolExecutionResult.artifact/files_changed/terminal_output → dispatcher mengembalikan result → loop child hanya mengambil `outcome.result.output` dan memasukkannya ke conversation. `sink: None` mencegah live artifact/terminal events dan field artifact tidak diemit. Final hanya satu report markdown subagent yang dipublikasikan.

**Dampak:** diff/output artifact yang dibuat child hilang dari UI dan persisted artifacts, file changes tidak tercatat dalam execution trace. Parent dapat mengatakan ada hasil tool tetapi pengguna tidak bisa meninjau bukti tool itu seperti execution parent.

**Fix minimal:** child mengemit semua artifact hasil execution serta scoped tool start/end melalui child-specific identity; forward sink dengan subagentId agar event tidak bercampur parent. Pertahankan artifact IDs pada tool progress/result dan files_changed, bukan hanya prose report.

**Test pembukti:** child menjalankan edit_file + command; assert diff/terminal artifact ada pada event stream dan persisted session, IDs unik antar dua child, progress final menunjuk artifact yang benar.

### RT-15 — M — Snapshot replay mengabaikan `segments`/`preamble` sehingga urutan multi-turn runtuh

**Lokasi:** `src-tauri/src/agent/runner.rs:1767–1787`; data model `src-tauri/src/agent/types.rs:303–315`; terkait `src/hooks/useAgent.ts:260–280,433–460`, `MessageRow.tsx:157–185`.

**Trigger / call path:** satu assistant message berisi text sebelum tools, text setelah tools, lalu tools lagi dan final → UI menyimpan urutannya pada segments dan memindah opening content ke preamble → next request / manual reflection memanggil append_history_message yang hanya memakai message.content dan agregat tool_calls. Semua tool dipindah ke satu assistant invocation, seluruh result ditempatkan setelahnya; preamble hilang. `arguments_raw` juga tidak digunakan untuk membedakan tool streaming belum selesai.

**Dampak:** provider menerima kronologi yang berbeda dari live run dan tampilan UI; rationale/instruksi dalam preamble hilang, intermediate text yang seharusnya mengikuti tool result muncul sebelum semua tool result. Manual reflection menganalisis bukti yang berbeda dari automatic reflection yang memakai native conversation asli. Types sudah menyimpan fields ini, jadi ini bukan masalah serde stripping.

**Fix minimal:** gunakan canonical provider-turn records terpisah dari presentation message, atau replay segments secara berurutan dan emit assistant→paired tool results per tool-bearing segment. Pertahankan preamble legacy hanya sebagai fallback, jangan duplikasi text yang sudah ada dalam segments. Kombinasikan normalisasi RT-04 agar pending tidak menjadi call sah.

**Test pembukti:** capture native live conversation dua tool turns, simpan/reload message, replay, dan bandingkan urutan/content/call-result pair secara semantik. Manual reflection harus melihat resep/tool sequence yang sama dengan auto reflection.

### RT-16 — M — Kontrak council menerima dua scope independen tetapi registry hanya melindungi A

**Lokasi:** `src-tauri/src/agent/runner.rs:1331–1343,1510–1515,1566–1582`; terkait `src-tauri/src/lib.rs:40–60`; `src/hooks/useAgent.ts:375–377,792–828`.

**Trigger / call path:** invoke send_council_message dengan requestB.runId/sessionId berbeda dari A → begin hanya A → event B menggunakan scope B → Done menggunakan scope A. UI normal saat ini memang mengisi scope sama; tidak ada validasi backend yang mensyaratkannya, walau type memberi dua set ID penuh. Jika sesi B punya run aktif, candidate council B tidak tercatat pada registry B.

**Dampak:** B events dibuang hook normal atau diterima scope run lain; session concurrency invariant tidak berlaku untuk candidate B dan user dapat melihat council separuh kosong. Ini bug boundary IPC, bukan klaim UI normal selalu mengirim mismatch.

**Fix minimal:** validasi kedua run/session IDs identik sebelum begin, atau hilangkan scope B dari kontrak dan turunkan semua event scope dari shared council envelope. Registry cukup satu lease bila council satu run; jangan mencoba mendaftarkan dua run tanpa lifecycle terdefinisi.

**Test pembukti:** request mismatch ditolak sebelum provider/network, shared scope menghasilkan kedua candidate events + tepat satu done, dan unrelated active session tidak menerima event council.

## Reflection: temuan dan batas residual

### RT-17 — M — Registry manual review dan run tidak mutually exclusive secara atomic

**Lokasi:** `src-tauri/src/agent/runner.rs:673–692,712–743,1303–1327,1608–1623,1631`; terkait `src-tauri/src/lib.rs:31–35`.

**Trigger / call path:** review_session memeriksa active registry lalu melepaskan lock sebelum load_session await dan reservasi reviews → concurrent begin dapat memasukkan run session yang sama. Bahkan setelah reservasi review, begin hanya melihat active, tidak melihat reviews. Auto-reflection sengaja detached setelah Done; manual review tampaknya dimaksudkan hanya ketika idle (pesan `Stop the active run before reviewing this session`). Token reflection selalu baru dan tidak terdaftar pada abort.

**Dampak:** manual review dapat berjalan serentak dengan live mutation/run yang kebijakan awalnya ingin menolak; Stop tidak dapat menghentikan provider review/skill write. Ini tidak otomatis menyebabkan lost update generated skill karena SKILL_WRITER_LOCK + snapshot checks melindungi sesama reflection writer. API/plugin/dua caller cukup untuk race; UI tunggal punya guard sendiri tetapi bukan backend invariant.

**Fix minimal:** gunakan satu registry lifecycle dengan reservasi manual-review/run atomik; tentukan bahwa auto-review snapshot detached boleh coexist jika memang fitur yang diinginkan. Simpan review token/handle dan endpoint abort review eksplisit bila operator diharapkan dapat menghentikannya; jangan menautkan token ke run selesai tanpa mempertimbangkan semantics.

**Test pembukti:** barrier antara active-check dan disk-load lalu concurrent begin; hanya satu operasi eksklusif diterima. Cancel manual review saat provider menunggu harus mencegah skill commit dan release lease. Auto-review coexist diuji sesuai policy terpisah.

### Batas residual / uncertain — jangan diklaim sudah direproduksi

1. **M — Commit skill dan record session bukan satu transaksi.** `runner.rs:1625–1636` memanggil reflect (yang dapat commit skill `reflection.rs:284–303`) sebelum record_self_improvement. Jika turn/session dihapus saat review menunggu, storage:391–400 gagal setelah skill terlanjur dibuat. Hasilnya error UI dengan skill tetap ada tanpa asosiasi badge. Schedule ini didukung source; apakah produk mengizinkan skill bertahan setelah sesi dihapus adalah keputusan policy. Fix: cek lease/revision target sebelum commit, atau return committed result walau attachment session gagal; jangan rollback skill concurrent lain secara buta. Test: delete turn/session selama provider delay lalu periksa skill dan outcome yang konsisten.
2. **M — Reparse/compare checks bukan filesystem-handle atomic guarantee.** Reflection sudah menolak static symlink/junction dan memeriksa ulang sebelum rename, tetapi `checked_skill_path`/compare lalu pathname rename (`reflection.rs:353–368`) menyisakan race lokal terhadap attacker yang menukar parent setelah check terakhir. `read_generated_skill:713–720` juga stat→read dengan pathname; file bisa tumbuh/berubah setelah size check. Existing tests:524–552 menukar junction selama provider request, bukan di jendela check→rename. Belum ada eksploit runtime, jadi jangan mempromosikan menjadi arbitrary-write confirmed. Bila threat model mencakup proses lokal adversarial, gunakan verified directory/file handles dan no-follow/reparse-safe operations + hard read bound.
3. **M — Timeout 45s reflection bukan deadline keras seluruh I/O.** `reflection.rs:164–170` melakukan load existing sebelum timeout select; `write_skill_atomic:325–368` menunggu stat/read/write/sync/rename tanpa membungkus tiap await di deadline. Check sebelum/sesudah tidak menginterupsi I/O yang hang. Existing timeout test hanya provider delay. Dampak pada slow/network filesystem memerlukan verifikasi lingkungan. Test inject stalled filesystem abstraction; enforce budget untuk keseluruhan operation, dokumentasikan cleanup yang tidak dapat dibatalkan di filesystem blocking.
4. **L — run registry cleanup tidak RAII dan Done mendahului finish.** lib.rs:33–35/55–59 hanya finish setelah future kembali normal; panic/cancellation JoinHandle akan melewatkannya. runner:1303/1579 emit Done sebelum registry remove; immediate next begin/review dapat mendapat already active sementara UI sudah idle. Tidak ditemukan panic konkret di scope, jadi permanent registry leak bukan confirmed user trigger. Fix RunLease drop guard dan release sebelum final externally-visible Done sesuai lifecycle yang dipilih. Test injected panic/future abort dan event callback segera begin berikutnya.
5. **M — shutdown ownership background process tidak terdefinisi.** `kill_on_drop(false)` (tasks:107), global OnceLock, monitor detached, dan lib run builder tidak mempunyai exit cleanup/recovery registry. Proses daemon dapat survive app exit dan tidak didaftarkan ulang pada start. Survive daemon bisa desain sengaja; yang hilang adalah explicit persist policy, discovery, dan verified cleanup. Test nyata app exit dengan grandchild diperlukan sebelum mengklaim semua server orphan; parent punya native gates.

## Hal yang diperiksa tetapi tidak dilaporkan sebagai bug

- Mutex sinkron active/reviews runner digunakan dalam region pendek tanpa menahan guard melintasi await; tidak ditemukan lock-order deadlock konkret pada dua registry tersebut. Masalahnya atomic policy antar registry, bukan sekadar penggunaan std::sync::Mutex.
- `begin` menolak runId duplikat dan satu active run per session dalam **active registry yang sama**. `abort` membatalkan token, tidak menghapus entry terlalu awal. `finish` tidak mengambil token baru secara keliru pada alur normal.
- Tool UI identity parent sudah memasukkan `turns` dan menggunakan `call.stream_index` pada final update. Bug lama `stream_0` collide lintas turn tidak ditemukan pada normal live run; risiko saat ini retry attempt dan history serialization.
- Cancellation pada normal provider retry/backoff dan parent MCP call sudah di-select. Tidak menuduh provider/MCP call normal sepenuhnya uncancellable; `join_all` masih tertahan oleh tool yang tidak menerapkan cancel seperti send_input.
- Workspace subpath child memakai resolve_workspace_path + is_dir, bukan unchecked join traversal. Tidak melaporkan traversal subpath lama yang sudah tidak ada. Empty-workspace temp fallback tetap masalah RT-12.
- Reflection menghapus seluruh extra_body sebelum completion, mengirim tools kosong, meredaksi active provider api/header credentials, membatasi transcript, menolak manual/unmanaged skill overwrite, mempertahankan authored body, dan melakukan snapshot comparison di bawah satu writer mutex. Ini membatasi temuan lost-update **antar reflection writers**; RT-05 menyasar mutation native file tools yang memakai helper berbeda.
- Auto reflection memakai slice sejak user terakhir dari native conversation; snapshot lama seluruh sesi tidak disalin sembarangan. Manual review menolak run status failed/cancelled/running yang teridentifikasi. Perbedaan bukti auto/manual yang tersisa adalah replay RT-15.
- Tidak menafsirkan native run successful sebagai semua tools sukses; agent bisa sah memperbaiki kegagalan tool lalu menyelesaikan turn. Karena itu tidak mengusulkan menggagalkan seluruh run hanya karena pernah ada satu tool error.

## Gap test yang paling bernilai bagi parent

Prioritas gates/regression yang disarankan, **belum dijalankan**:

1. RequestReview forced-write + child policy inheritance (RT-01/11).
2. send_input backpressure → kill dan parent Stop (RT-02); bounded no-newline output (RT-03).
3. Abort partial tool args → request berikutnya valid (RT-04), retry attempt rollback (RT-10), live/save/reload replay equivalence (RT-15).
4. Concurrent edits barrier (RT-05), task owner isolation (RT-06), EOF-before-exit dan descendant pipe handling (RT-07/08).
5. Synthesis provider error/cancel status child (RT-13), child artifact persistence (RT-14), no-workspace child sandbox (RT-12).
6. Reflection/run reservation race (RT-17), delete-session-during-review outcome, deadline/file race residual.

Existing tests berguna tetapi tidak membuktikan kasus ini: task tests hanya spawn completion, status killed setelah 200ms, dan not-found; tidak memeriksa proses tree sudah mati atau stdin lock saturation. Runner concurrency test hanya synthetic sleep. Reflection tests mempunyai coverage parsing/redaction/static reparse/concurrent reflection writer yang nyata dalam source, tetapi tidak menutup native review registry race, commit-to-session association, atau jendela handle-level TOCTOU.

## Self-check kualitas laporan

| Sumbu | Nilai | Bukti / gap spesifik |
|---|---|---|
| Accuracy | 4/5 | Setiap confirmed finding ditautkan ke branch/call path source; belum ada reproduksi runtime karena explicit read-only/gates parent. |
| Completeness | 4/5 | Seluruh 5008 baris scope dibaca; app-exit/process-tree behavior tetap memerlukan native execution parent. |
| Clarity | 4/5 | Severity, trigger, impact, minimal fix dan regression dipisahkan; temuan lintas boundary memerlukan file terkait di luar lima scope. |
| Actionability | 4/5 | Ada patch direction kecil dan tests spesifik tiap finding; tidak ada patch source atau tests baru karena tahap read-only. |
| Conciseness | 4/5 | Report panjang sesuai deep review; RT-04/10/15 sengaja dipisah karena failure mode dan fixes berbeda walau history pipeline saling terkait. |

Rata-rata **4.0/5**. Perbaikan utama: parent memverifikasi RT-02/04/05 dengan regression deterministik, menetapkan semantics owner/daemon, lalu menangani native policy enforcement. Self-check: penilaian tidak menyatakan runtime telah terbukti, dan tidak menghukum ketiadaan build yang memang dilarang pada delegasi ini.
