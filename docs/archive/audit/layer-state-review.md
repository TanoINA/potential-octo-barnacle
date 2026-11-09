# Audit layer React state / hooks / IPC / App

## Ruang lingkup dan batas bukti

Audit statis, READ-ONLY terhadap source. Tidak menjalankan test, build, lint, aplikasi, provider, atau command IPC. Satu-satunya write audit adalah dokumen ini. `docs/audit-evidence-tools-2026-11-08.md` tidak dibaca/diubah.

Root terverifikasi: `D:/AetherAgent`. `git status --short` awal hanya menunjukkan file evidence tersebut sebagai untracked. Tidak ditemukan `AGENTS.md` pada root, `D:/`, `src/`, `src/hooks/`, dan `src/types/` yang berlaku pada scope. `package.json:9-10,22-23` mendeklarasikan build `tsc && vite build`, test Vitest, React 19; tidak ada klaim verifikasi runtime.

Cakupan utama dibaca penuh:
- `src/hooks/useAgent.ts` (state, startup, semua callback, event listener, persistence, navigation, undo/rollback/edit/retry).
- `src/hooks/useSpeechRecognition.ts`, `src/hooks/useModalAccessibility.ts`, `src/hooks/useIsNarrowViewport.ts`.
- `src/types/agent.ts`, `src/App.tsx`, `src/main.tsx`.
- `src/hooks/useAgent.reflection.test.tsx`, `src/hooks/useSpeechRecognition.test.tsx`, `src/App.test.tsx`, `src/test/setup.ts`.

Penelusuran wiring terarah: `ChatCanvas.tsx` (queue, composer, attachments, native listeners, slash commands), `MemoryModal.tsx`, `ModelPicker.tsx`, `MessageRow.tsx`, `CouncilView.tsx`, `Sidebar.tsx`, `InteractiveTerminal.tsx`, `SettingsModal.tsx`, `ErrorBoundary.tsx`; test queue/commands/reflection dibaca terarah. Backend hanya dibaca untuk mengonfirmasi kontrak IPC: `src-tauri/src/lib.rs`, `agent/runner.rs`, `agent/storage.rs`, `agent/types.rs`, `agent/tools.rs`, `agent/provider.rs`. Bukan audit backend menyeluruh.

Severity: **C** kritis; **H** data/eksekusi bisa salah sesi/workspace atau kehilangan state penting; **M** inkonsistensi/fungsi gagal pada jalur nyata; **L** kualitas/error UX terbatas. Semua trigger berikut merupakan interleaving yang diturunkan dari source, **belum direproduksi dengan eksekusi**.

## Temuan

### ST-01 — H — Run yang sudah ditinggalkan tetap dapat dimulai; catch run lama membatalkan tracking run baru

**Lokasi:** `src/hooks/useAgent.ts:780-784,830-844,868-882,915-935`.

**Trigger/call path:** `sendMessage(A)` memasang `currentRunRef=RA`, kemudian menunggu `persistData`. Sebelum save selesai, pengguna New/Switch/Delete. `detachCurrentRun` mengosongkan run ref dan mengirim `abort_agent(RA)`. Karena backend belum menjalankan `begin(RA)`, abort tidak mencatat tombstone (`runner.rs:695-703`). Setelah save selesai, continuation lama tetap memanggil `send_message` / `send_council_message`, tanpa mengecek navigation, origin session, atau run identity. Backend mengambil cwd global pada saat command diproses (`lib.rs:25-34,47-58`), bukan cwd saat user mengirim.

Cabang kedua: sambil promise run A belum settle, pindah B dan mulai RB. Bila promise A reject, catch A mengosongkan `currentRunRef` dan `streamingMessageIdRef` tanpa memastikan keduanya masih milik RA. UI B menjadi non-streaming dan menolak event RB selanjutnya. Catch juga mempersist snapshot sesi yang *saat itu* aktif.

**Dampak:** task A bisa berjalan tanpa UI/Stop, bahkan memakai workspace B/empty; kegagalan A merusak monitoring B.

**Fix minimal:** capture `{sessionId, navigation, runId, workspace}` pada awal; setelah setiap await sebelum start dan di catch/finalization, validasi identity. Cancellation sebelum start harus mencegah start (token/tombstone/lifecycle eksplisit), bukan hanya best-effort abort. Jangan null/reset run lain.

**Regresi:** deferred `save_session(A)` → New/B → resolve save; assert tidak ada send RA. Deferred send A → mulai RB → reject A; RB tetap streaming dan chunk/done RB tetap diterima.

### ST-02 — H — Switching/startup tidak menjadi transaction; input/queue boleh menulis sesi lama ketika load sedang berlangsung

**Lokasi:** `src/hooks/useAgent.ts:346-369,703-711,966-991`; `src/components/ChatCanvas.tsx:361-383`; `src/App.tsx:117-162,272-275`.

**Trigger/call path:** A sedang tampil; switch B memanggil detach, snapshot/save A, dan `setIsStreaming(false)` tetapi tidak memasang navigation lock atau mengganti current session sampai load selesai. Composer tetap aktif; `sendMessage` tidak mengecek `isLoading/navigation pending`. Prompt baru masuk A setelah snapshot A diambil. Load B kemudian mengganti seluruh data dan refs; run/prompt A hilang dari view dan final save-nya tidak dijamin.

Ini juga terjadi **otomatis**: A streaming dengan queued prompt; switch B mengubah streaming true→false sementara `currentSessionId` masih A. Effect queue `ChatCanvas:368-379` menganggap run selesai dan mengirim queued prompt A. Reset queue baru terjadi ketika ID B sudah terpasang; saat itu terlalu lambat. Pada New yang langsung mengganti ID, dua effect masih membaca queue closure render yang sama: `setQueuedPrompts([])` tidak membatalkan dispatch effect di render tersebut.

Startup serupa: `setIsLoading(false)` dipanggil tanpa menunggu `load_session`; settings sudah tersedia sehingga user dapat mengirim ke sesi sementara, lalu restore latest menggantinya karena send tidak menambah navigation generation. Kasus klik kembali A saat switch A→B pending juga tidak membatalkan switch: early return `sessionId === sessionIdRef` berada sebelum increment generation.

**Dampak:** prompt terkirim tanpa aksi user setelah navigasi, ke sesi lama/baru yang salah; load menimpa pesan baru; UI menganggap switching selesai padahal belum.

**Pendalaman initial loading:** `Promise.allSettled(...).then` (`324-370`) tidak mengembalikan promise `load_session`: pemanggilan di `349` dibiarkan berjalan dan `369` langsung mematikan loading. Minimal ubah initialization menjadi async transaction yang `await load_session` lalu `await set_workspace_cwd`, dengan `active/navigation` check di setiap boundary dan `finally` hanya menutup generation loading yang dimilikinya. Hindari `setIsLoading(false)` initialization lama yang mematikan indikator compaction baru. Render composer blocked/read-only hingga transaction selesai, atau bila mengizinkan send awal, perlakukan send sebagai pembatalan restore generation dan commit sesi sementara secara eksplisit. Jangan sekadar menambah spinner tanpa menahan `sendMessage` karena callback programmatic/queue tetap bisa berjalan.

**Fix minimal:** transaction navigation yang menahan send dan queue dispatch hingga load + workspace commit selesai; generation tetap diperbarui untuk intent kembali ke A. Queue entry dan transition harus memiliki session/run identity; hanya dispatch setelah terminal event origin, bukan sembarang perubahan boolean. Startup restore wajib ditunggu/dibatalkan ketika ada mutasi user.

**Regresi:** A→B deferred load + send; A running + queue + Switch/New/Clear; A→B→A sebelum B selesai; delayed initial load + send. Assert tidak ada auto-send silang dan pesan tidak hilang.

### ST-03 — H — Edit/retry melanjutkan prompt A ke sesi B dan dapat membawa buffered delta run lama

**Lokasi:** `src/hooks/useAgent.ts:232-281,1437-1499,1501-1538`; `src/components/ChatCanvas.tsx:1102-1123`.

**Trigger/call path:** editAndResend(A) memotong history, `await persistData(next)`, lalu memanggil `sendMessage(clean, attachments)` yang membaca `dataRef` terbaru. Switch/New di tengah await tidak diblokir `sessionOperationRef`; continuation mengirim teks/attachments A ke B. Slash `/retry` mempunyai pola sama: `onUndoTurn().then(onSendMessage(...))` tanpa origin check. `/undo` dapat mengisi composer B dengan hasil A.

Saat retry/edit memutus run aktif, kode hanya men-null run ref; tidak flush/clear `textBufferRef`, `councilBufferRef`, `terminalBufferRef`, frame, atau streamingMessageId lama. Jika RAF tertunda (misalnya window background), run baru bisa terpasang sebelum flush; flush memakai messageId **saat flush**, sehingga delta lama dapat menempel ke assistant baru. Jika RAF jalan saat persiapan edit, delta bisa dibuang karena message lama sudah dipotong. Keduanya bukan buffering beridentitas run.

**Dampak:** prompt/attachments lintas sesi, potongan jawaban salah turn, kehilangan output atau history.

**Fix minimal:** capture origin session+navigation untuk edit/undo/retry dan validasi sesudah await; gunakan satu helper terminate/detach yang mem-flush untuk origin atau membuang seluruh buffer dan RAF secara eksplisit. Simpan identity pada buffer; clear/flush sebelum memasang run berikut.

**Regresi:** pause save edit/undo A → switch B → resolve; tidak ada send/isi composer B. Hold RAF → enqueue old chunk → retry → start run baru → flush; teks lama tidak masuk jawaban baru.

### ST-04 — H — Lock operasi sesi tidak mencakup Clear/Delete/navigation; hasil compaction atau save lanjutan dapat menghidupkan data yang sudah dihapus

**Lokasi:** `src/hooks/useAgent.ts:999-1015,1130-1135,1293-1324,1328-1379,1384-1434`; `src-tauri/src/agent/storage.rs:367-369,424-472,532-541`.

**Trigger/call path:** compaction A memasang `sessionOperationRef=true`, save A, lalu menunggu `compact_session`. `/clear` tidak mengecek lock dan tidak menaikkan navigation; ia mengosongkan history A dan menyimpan snapshot kosong. Ketika compaction mengembalikan data lama, guard hanya mengecek navigation+session yang belum berubah, sehingga data lama dipasang lagi. Save berikutnya mengabadikannya. Clear saat undo save gagal juga dapat ditimpa rollback `mutateData(previous)` karena guard undo hanya mengecek session ID/run.

Delete hanya menunggu *nilai promise queue saat itu*, bukan memasukkan delete sebagai barrier/operation ke queue. Pending switch ke B tidak dibatalkan ketika B dihapus sementara A masih current. Load B yang sudah membaca sebelum delete dapat kemudian memasang B; save berikutnya membuat file B lagi. Native mutex mencegah penulisan simultan tetapi tidak mencegah snapshot/continuation lama sesudah delete; save tidak punya revision/tombstone.

**Dampak:** Clear tampak berhasil lalu history kembali; sesi yang dihapus dapat muncul kembali; mutasi user hilang.

**Fix minimal:** satu operation coordinator per session untuk save, compact, clear, rename, pin, delete, load intent; clear/delete menaikkan generation dan membatalkan hasil operasi terdahulu. Delete memasang tombstone/revision dan menolak stale saves. Navigasi ke target yang dihapus harus diinvalidasi.

**Regresi:** deferred compact → Clear → resolve; undo save reject setelah Clear; delayed load B → delete B → resolve load → navigate away. History tetap kosong dan B tidak tercipta kembali.

### ST-05 — H — Cwd global IPC tidak terikat transaction sesi; workspace UI/persisted/run dapat berbeda

**Lokasi:** `src/hooks/useAgent.ts:359-366,948-955,980-991,1176-1201,1229-1258`; `src/types/agent.ts:198-216`; `src-tauri/src/lib.rs:25-34,346-374`.

**Trigger/call path:** switch menampilkan normalized session/cwd **sebelum** await `set_workspace_cwd`; send tidak ditahan. `send_message` menyalin cwd global backend, yang masih bisa milik sesi sebelumnya. `set_workspace_cwd` sendiri melakukan storage await sebelum mengubah mutex, sehingga beberapa switch/reset/select dapat selesai terbalik. Fire-and-forget reset/restore juga menelan failure: UI kosong/B tetapi backend masih A.

`selectWorkspace` menunggu dialog dan invoke, kemudian memutasi *session yang current saat resolve*, bukan session origin. Pilihan folder A yang terlambat dapat menimpa metadata B. `clearWorkspace` mempunyai pola sama. Keduanya tidak mempersist sesi setelah perubahan; pengguna memilih/menghapus workspace pada sesi berisi pesan lalu menutup aplikasi sebelum aksi lain akan memuat workspace lama. Refresh git juga tidak memiliki workspace generation; respons A dapat memasang status/branch A setelah pindah B atau clear.

**Dampak:** command agent bisa menulis proyek salah; workspace hilang setelah restart; commit/status UI tidak sesuai proyek yang terlihat.

**Fix minimal:** kirim cwd eksplisit terverifikasi dalam request run dan command workspace-sensitive, bukan mengambil mutable global saat dispatch. Serialize dan guard set-workspace dengan session/navigation identity; baru enable send setelah sukses. Persist metadata setelah select/clear berhasil; tolak/label respons git yang berasal dari workspace lama. Tampilkan failure reset/restore, jangan swallow.

**Regresi:** deferred set cwd A/B dengan resolve terbalik; send di tengah switch; dialog folder A resolve setelah switch B; select/clear → reload tanpa send; delayed git A resolve sesudah clear/B.

### ST-06 — H — Pelanggaran requirement lifecycle: sekadar switch/new membatalkan run aktif

**Lokasi:** `src/hooks/useAgent.ts:173-174,375-377,915-935,966-970`; `src/hooks/useAgent.reflection.test.tsx:96-116`; `src-tauri/src/agent/runner.rs:673-692`.

**Trigger/call path:** mulai RA pada A lalu buka riwayat B. `switchSession` selalu `detachCurrentRun`, menandai assistant/tools/subagents cancelled dan abort RA. Listener hanya menerima satu current run **dan** viewed session. Tidak ada map active runs/data per session, tidak ada reattach/replay. Native registry justru mendukung run sesi berbeda.

**Dampak:** membaca sesi lain menghentikan task tanpa explicit Stop; UX multi-session/background-run tidak tersedia. Parent mengonfirmasi bahwa ini **bertentangan dengan requirement eksplisit user**, jadi prioritas perbaikan H, bukan opsi desain yang dapat dipertahankan dengan confirmation saja. Perilaku sekarang sengaja dikunci test tetapi test tersebut salah kebijakan dan harus diganti. Background shell tasks (`backgroundTasks`) terpisah dan bukan bukti dukungan background agent runs.

**Desain perubahan minimal (READ-ONLY, belum diimplementasikan):**
1. Pisahkan `viewedSessionId` dari lifecycle run. Buat `liveSessionsRef: Map<sessionId, LiveSnapshot>`; setiap snapshot minimal menyimpan `data`, `revision`, `runId`, `assistantId`, `phase: starting|running|stopping|finished`, `workspaceCwd`, `failed`, `stopRequested`, `startedAt`, `subagents`, `activeSkills`, serta buffers `{runId,assistantId,text,council,terminal}`. Ref global boleh menjadi cache/registry, tetapi jangan menjadikan viewed session sebagai event target.
2. Extract reducer/helper `mutateSession(sessionId, expectedRunId?, fn)` dan `flushRun(sessionId,runId)`. Listener tunggal route dari payload `{sessionId,runId}` ke live snapshot origin, memeriksa run identity; update React view hanya bila snapshot tersebut sedang dilihat. `done/error/tool/artifact/council/review` tetap diproses offscreen. `done` mem-flush buffer origin, finalisasi snapshot origin, dan persist snapshot origin, **bukan** `dataRef.current` sesi yang terlihat.
3. `switchSession/newSession` hanya mengubah projection view dan menyimpan snapshot; jangan panggil abort/cancelledMessage atau membuang buffer origin. Kembali ke sesi yang live gunakan snapshot memory, jangan `restoreSession` yang menormalisasi streaming menjadi cancelled. Disk load hanya untuk sesi tanpa live snapshot, dengan revision+navigation guard; respons disk lama tidak boleh mengganti snapshot yang berkembang selama await. Streaming/Stop/subagents/skills UI diderive dari snapshot viewed, bukan satu global boolean/ref.
4. Capture workspace/provider/request saat run dibuat dan kirim expected workspace eksplisit ke IPC (ST-05). Sebelum native begin, `phase=starting` harus punya cancel intent per run; hanya explicit Stop/Clear/Delete origin yang dapat menggagalkan start. Seluruh after-await/catch memasang hasil hanya pada snapshot+run asal. Jangan batalkan RA hanya karena New B.
5. Pertahankan queue persistence yang sudah ada sebagai langkah awal, tetapi enqueue immutable snapshot origin dengan revision; metadata/delete/compact juga harus masuk coordinator agar stale writes ditolak. Offscreen `done` wajib enqueue final save; tambahkan checkpoint throttled selama streaming agar window crash tidak kehilangan semua output sejak initial save. Evict snapshot hanya setelah run selesai, buffers kosong dan persist sukses; tombstone delete menolak queued/late writes. Registry/cache diberi total retention budget.
6. Kebijakan destructive: Stop/Clear/Delete mengacu `(sessionId,runId)` eksplisit dan menunggu/catat terminal completion, tidak memakai viewed refs yang dapat berubah. Navigasi normal tidak memiliki side effect cancellation. Queue entry juga menyimpan origin/options; auto-dispatch berdasarkan terminal event origin, bukan viewed `isStreaming` true→false. Mulai dari satu run per session (backend sudah menegakkan), beberapa session dapat berjalan paralel.

**Test kebijakan yang harus diganti:** `useAgent.reflection.test.tsx:96-116` sekarang mengharapkan cancelled-on-switch; ubah menjadi assert tidak ada `abort_agent`, assistant A tetap streaming saat view B, chunk/tool/artifact/done A tersimpan di disk A dan terlihat lengkap ketika kembali. Review A boleh selesai offscreen dan terikat origin.

**Regresi acceptance:** A running → view B/New C → chunk/tool/terminal/council/done A → kembali A; output lengkap, status benar, tanpa abort A. Jalankan RA dan RB bersamaan, out-of-order done/error, lalu verify disk masing-masing session tidak tercampur. Kembali A sebelum done tidak memuat stale disk di atas live snapshot. Explicit Stop A ketika sedang melihat B hanya mempengaruhi A. Delete A saat starting/running tidak menyisakan hidden run atau resurrected file. Hold RAF saat A offscreen dan verify budget/flush final. Perubahan source/test hanya dilakukan setelah parent memberi scope implementasi.

### ST-07 — M — Rename/pin menunggu queue tetapi tidak menjadi bagian queue; snapshot stale menimpa metadata baru

**Lokasi:** `src/hooks/useAgent.ts:218-229,1089-1104,1110-1124`; `src-tauri/src/agent/storage.rs:410-417,431-451,507-529`.

**Trigger/call path:** rename B noncurrent menunggu queue lalu menjalankan native rename. Sementara itu switch/load B sudah membaca title lama dan belum memasang state, atau save baru B tersusun dari snapshot lama. Callback rename bisa belum melihat B current sehingga tidak memperbarui `dataRef`; save berikutnya membawa title lama dan menimpa rename. Mutex native hanya mengurutkan operasi, tidak memvalidasi freshness title.

Pin mempunyai kasus lebih pasti: sesi B sebelumnya pinned; snapshot B lama dengan `pinned=true,pinnedAt=t` dibaca; unpin B menulis false/None; snapshot lama dipasang/disimpan setelahnya. `merge_pin_state` hanya mengambil disk bila incoming `pinned_at` kosong dan disk pinned. Incoming stale pin yang nonempty tetap menang atas disk unpinned, sehingga pin hidup kembali.

**Dampak:** rename/unpin tampak sukses lalu kembali; sidebar dan active title berbeda sampai reload/save.

**Fix minimal:** antrekan metadata mutation sebagai operasi, bukan hanya await tail; session revision atau field-level authoritative merge untuk title/pin. Terapkan hasil load hanya jika revision masih sesuai.

**Regresi:** snapshot load B tertahan → rename/unpin B → resolve load → save B; title/pin terbaru tidak berubah. Jangan melaporkan ini sebagai "backend tanpa mutex": backend sudah memiliki `SESSION_IO_LOCK`.

### ST-08 — M — Settings save rollback dan refreshModels membawa snapshot lama yang dapat membuang perubahan terbaru

**Lokasi:** `src/hooks/useAgent.ts:1018-1028,1053-1085`; `src/components/ModelPicker.tsx:112-123,126-139`.

**Trigger/call path:** `refreshModels` menyimpan `current` sebelum await network. Sambil fetch berjalan, user mengubah favorite/provider/settings; fetch selesai membangun seluruh `next` dari current lama lalu `updateSettings(next)`. Perubahan baru hilang. Beberapa `updateSettings` juga tidak serialized/versioned; bila save S1 gagal setelah S2 sukses, catch S1 me-rollback ref/state ke previous S0, padahal S2 sudah sah. `addCustomModel` menunggu global settings save lalu memilih model di session current saat selesai, sehingga pilihan custom A dapat masuk B jika user New saat menunggu.

**Dampak:** preference/provider config baru hilang, memory UI dan disk berbeda, session model salah sasaran.

**Fix minimal:** patch field menggunakan settingsRef terkini sesudah fetch, revision/CAS atau serialize setting writes; rollback hanya bila request masih latest. Capture session untuk efek samping selection setelah addCustomModel.

**Regresi:** deferred fetch + toggleFavorite/edit provider; dua save resolve/reject terbalik; custom model save A → New B → resolve.

### ST-09 — M — Provider reconnect reset hanya content/reasoning, bukan segments yang sebenarnya dirender

**Lokasi:** `src/hooks/useAgent.ts:379-398,260-281`; `src/components/MessageRow.tsx:53-57,158-178,264-270`; `src-tauri/src/agent/runner.rs:931-946`.

**Trigger/call path:** provider sudah menghasilkan parsial yang terflush ke segments, lalu retryable timeout mengirim notice `resetPartial=true`. Hook mengosongkan buffer/content/reasoning tetapi tidak segments/preamble/partial tool state milik attempt. Chunk replacement diappend ke segment lama. MessageRow dan copy memakai segments sehingga output gagal tetap terlihat/tersalin, sementara message.content/provider history berisi replacement saja.

**Dampak:** teks lama+baru terduplikasi, UI berbeda dari history yang diteruskan/export; retry tampak sukses tetapi answer yang terlihat salah.

**Pendalaman reset tools/council:** `runner.rs:871-916` membuat ulang `partial_tools` setiap attempt, tetapi ID tool berbentuk `turn_{turns}_stream_{index}` tanpa attempt. Flag `attempt_had_chunks` hanya ditandai content/reasoning (`885,889`), **bukan** delta tool. Jika provider hanya mengirim tool arguments parsial lalu timeout, notice dapat `resetPartial=false`; tool phantom dari attempt gagal tidak pernah dihapus. Jika attempt baru menghasilkan fewer/no tools, pending tool lama tetap di `message.toolCalls` dan segments; `done` normal hanya mematikan streaming, tidak menyelesaikan/membuang pending tool. Ini juga dapat memblokir review (`useAgent:664-667`) setelah jawaban dianggap completed.

`resetPartial=true` saat provider turn lanjut juga mengosongkan seluruh accumulated `message.reasoning` dan `content`, bukan hanya attempt berjalan, sementara segments/preamble/tool state sebelumnya tetap ada: data yang valid dapat hilang di representation canonical tetapi masih terlihat di segments. Karena itu fix `segments=[]; toolCalls=[]` global juga tidak benar—akan membuang tool history committed dari turn sebelumnya.

Hook notice tidak menyentuh council buffer/turn. Namun **backend Council saat ini tidak mengirim notice retry tersebut**: `run_council` memakai stream langsung per candidate (`runner.rs:1460-1507,1528-1575`), tanpa loop reconnect seperti regular run. Jadi tidak mengklaim confirmed Council retry-reset bug dari jalur yang belum ada. Bila retry candidate ditambahkan, scope reset harus mencakup `{candidate,attemptId}` serta content/reasoning/isStreaming candidate terkait; candidate lain dan chosenWinner yang committed tidak boleh ikut dihapus.

**Fix minimal:** checkpoint per provider attempt/turn dan event identity `turnId/attemptId` (serta candidate untuk Council); reset hanya segment/tool partial attempt gagal tanpa menghapus turn/tool yang sudah committed. Tandai had-deltas juga untuk tool atau kirim reset boundary tanpa tergantung content. Gunakan representation canonical tunggal untuk render/copy/content.

**Regresi:** stream old partial → flush → resetPartial → replacement → done; UI/copy/disk/provider payload semua hanya replacement untuk attempt tersebut. Tool-only partial timeout → retry menghasilkan plain text/fewer tools: tidak ada phantom pending tool dan review tidak terblokir. Timeout di turn kedua harus mempertahankan committed reasoning/text/tools turn pertama. Untuk Council, uji actual candidate done/error dan winner, jangan mengasumsikan event retry yang backend belum emit.

### ST-10 — M — Pemilihan pemenang Council sebelum candidate selesai membekukan content parsial/empty

**Lokasi:** `src/hooks/useAgent.ts:285-302,1274-1290`; `src/components/CouncilView.tsx:52-65,110-123`.

**Trigger/call path:** tombol Use response selalu enabled. Klik A ketika chunk A masih dibuffer/streaming. `selectCouncilWinner` menyalin `candidateA.content` saat itu ke `message.content` lalu persist; chunk berikutnya hanya menambah candidate, tidak memperbarui selected content. Bahkan setelah selesai jawaban utama dapat kosong/terpotong walau kartu A lengkap.

**Dampak:** turn berikutnya menerima assistant answer parsial/empty; persistence/export content tidak sama dengan candidate terpilih.

**Fix minimal:** disable winner selection sampai candidate complete dan flush pending, atau jadikan chosenWinner canonical lalu derive/synchronize content saat flush/done. Guard di hook juga, bukan hanya tombol.

**Regresi:** pilih sebelum RAF, pilih saat A belum selesai, A selesai sementara B masih streaming; content final terpilih selalu lengkap atau aksi ditolak.

### ST-11 — M — Buffer RAF tidak dibatasi; Clear tidak menghapus buffer/frame

**Lokasi:** `src/hooks/useAgent.ts:175-178,237-253,279-300,311,403-417,482-484,507-516,999-1015`.

**Trigger/call path:** background/minimized WebView menunda RAF sementara stream terus mengappend text/reasoning/council/terminal. Tidak ada byte cap, time-based fallback, atau per-run retention budget pada buffer/state. Content assistant disimpan juga dalam segments. `clearCurrentSession` membuang messages/artifacts tetapi tidak buffer/frame; bila RAF tetap tertunda, send berikut memakai buffer yang sama (identity problem ST-03).

**Batas klaim:** output `run_command` native **sudah** dibatasi 1 MiB dan delta setelah cap tidak dikirim (`tools.rs:21,1202-1233`); parser SSE punya batas *event*, dan nonstream response juga capped. Jadi ini bukan klaim satu command terminal native pasti menghasilkan unlimited bytes. Kekurangannya adalah tidak ada total UI/run/session budget untuk akumulasi text, banyak tool/artifact, serta RAF backlog; satu frame bukan batas memori.

**Dampak:** memory pressure dan flush/copy/serialization besar dapat membekukan UI pada run panjang; Clear tidak menjamin membuang transient output.

**Fix minimal:** batasi queued bytes/retained bytes secara eksplisit, flush timer fallback saat hidden, simpan bulk output ke file dengan preview; Clear/terminate wajib cancel RAF dan reset seluruh buffer identity.

**Regresi:** fake RAF tidak dieksekusi, banyak chunks/artifacts/commands, ukur budget yang dipertahankan; clear → run baru → RAF lama, tidak ada delta lama. Tidak ada klaim hasil benchmark pada audit ini.

### ST-12 — M — Listener IPC tidak punya readiness/error path; async native drop cleanup bocor di StrictMode

**Lokasi:** `src/hooks/useAgent.ts:374-377,622-627,629-654`; `src/components/ChatCanvas.tsx:680-719`; `src/main.tsx:7-12`.

**Trigger/call path:** registrasi `listen` async tidak di-await sebelum send enabled dan rejection tidak di-catch. Backend dapat menerima run sebelum listener terpasang (terutama cold start), sehingga chunk/done awal tidak tersinkron; jika registrasi gagal, UI dapat tertahan streaming dan promise rejection tidak muncul di toast. Cleanup hook utama memang menunggu promise sebelum unlisten (benar untuk late-resolution success), tetapi chain yang reject juga tidak ditangani.

Native drag-drop memakai variabel `unlisten` yang baru diisi setelah await import/registration. Unmount/StrictMode cleanup sebelum itu hanya melihat undefined; ketika setup selesai tidak ada disposed guard untuk langsung unlisten. Listener pertama bertahan setelah mount kedua dan dapat mengirim inspect_paths ganda/update ke instance yang sudah dilepas. Main menjalankan React.StrictMode.

**Dampak:** hilang event akhir/run tersangkut, duplikasi pekerjaan drop dan listener leak pada lifecycle tertentu.

**Fix minimal:** registration status/error surfaced dan send menunggu ready atau replay/reconcile native run state; catch rejection saat register/dispose. Untuk drag-drop pakai disposed flag dan late disposal promise; guard async handler setelah await dan kaitkan ke session origin.

**Regresi:** delayed listen + immediate send; listen reject; StrictMode mount→cleanup sebelum registration resolve→remount; assert listener aktif tunggal dan unlistener akhirnya terpanggil.

### ST-13 — M — App shortcut menembus modal dan editor memory tidak terikat workspace asal

**Lokasi:** `src/App.tsx:45-71,117-162,227-234`; `src/hooks/useModalAccessibility.ts:37-44,63-67`; `src/components/MemoryModal.tsx:30-50,65-74`; `src/hooks/useAgent.ts:1551-1557`; `src-tauri/src/lib.rs:285-290,312-328`.

**Trigger/call path:** App tetap memproses Ctrl+N/Ctrl+K/Ctrl+, dari input modal; hook aksesibilitas hanya menghentikan Escape/Tab, bukan app shortcut, dan App tidak mengecek modal-open/defaultPrevented/target. Ctrl+N di editor Memory diam-diam mengganti session/workspace, bahkan ketika editor dirty. Beberapa boolean modal dapat true sekaligus; masing-masing memasang capture focus trap sendiri.

MemoryModal effect tidak bergantung `workspaceCwd` dan read/save API tidak membawa expected workspace; save memilih cwd global saat invoked. Bila pending switch B selesai setelah memory A terbuka, isi A tetap tampil tetapi save menuju AGENTS.md B. Ctrl+N sendiri mengubah cwd menjadi empty sehingga save gagal, namun dirty content masih tampak terhubung ke editor workspace; tidak ada invalidation.

**Dampak:** navigasi/destructive cancellation dari dalam dialog, focus traps bersaing, serta overwrite instruksi proyek lain pada interleaving workspace tersebut.

**Fix minimal:** central top-modal/shortcut policy; jangan jalankan navigation shortcut saat dialog/editor terbuka tanpa consent. Capture workspace identity pada Memory editor, reload/confirm dirty ketika workspace berubah; read/save IPC menerima expected workspace dan menolak mismatch.

**Regresi:** Ctrl+N di dirty Memory/Settings, Ctrl+K lalu Ctrl+, multiple modal Escape/Tab; delayed switch B resolve saat Memory A terbuka, save harus ditolak/terikat A dan tidak menyentuh B.

### ST-14 — M — Rollback/edit persisten gagal tetapi UI tetap memakai history terpotong tanpa pemulihan

**Lokasi:** `src/hooks/useAgent.ts:1422-1434,1487-1495` (bandingkan rollback undo `1372-1375`); `src/App.tsx:260-270`.

**Trigger/call path:** rollback/edit optimistically mengganti data sebelum save. Disk full/permission/IPC failure ditangkap, return false, tetapi tidak mengembalikan previous. Hanya undo punya best-effort rollback. UI sekarang menampilkan history terpotong padahal disk masih lengkap. Lanjut send/save sesudah storage pulih akan menimpa disk dengan truncation yang semula gagal.

**Dampak:** error toast bukan jaminan operasi dibatalkan; user dapat kehilangan tail history pada aksi berikutnya. Edit gagal juga tidak mengembalikan state run/turn sebelumnya.

**Fix minimal:** rollback conditional pada session+navigation+revision yang sama, atau commit UI setelah save berhasil. Jangan menimpa perubahan yang dibuat user selama await. Beri status unsaved yang persistent bila sengaja mempertahankan edit lokal.

**Regresi:** fail save rollback/edit → verify UI/disk tetap konsisten; switch/new atau Clear selama failure tidak boleh ditimpa rollback stale.

### ST-15 — L — Jalur speech/initial-load/search error belum konsisten terlihat di UI

**Lokasi:** `src/hooks/useSpeechRecognition.ts:35-57,68-76,79-99`; `src/components/ChatCanvas.tsx:840-844,2158-2170`; `src/hooks/useAgent.ts:343-361,1176-1178,1220-1226`; `src/App.tsx:260-269`.

**Trigger/call path:** speech unsupported/permission denied mengisi `isSupported/error` tetapi ChatCanvas hanya mengambil `isListening/toggleListening`; tombol mic tetap bisa diaktifkan tanpa penjelasan. Cleanup speech hanya abort; handler instance lama tidak dilepas/disposed, sehingga queued result/end dari instance lama saat lang berubah/StrictMode masih dapat memanggil transcript terbaru atau menurunkan listening instance baru. Test speech hanya final/interim/end, tidak lifecycle/error.

Startup `list_sessions` failure tidak memiliki else, `load_session` restore catch kosong; app dapat menampilkan New Chat tanpa memberi tahu existing session gagal dibaca. `deepSearch` mengembalikan [] pada failure, menyamakan backend error dengan tidak ada hasil. `selectWorkspace` membuka dialog di luar try, sehingga dialog rejection lolos dari error UI. Toast global belum `role=alert`/live-region dan tombol dismiss belum accessible name.

**Dampak:** fitur terlihat diam/tidak didukung, data existing tampak hilang sementara, screen reader dapat melewatkan error. Bukan bukti actual data dihapus.

**Fix minimal:** konsumsi error/support speech dan disable/label mic; cleanup handler instance lokal dan disposed guard; surface startup/list/load/search/dialog failures, bedakan empty/error; beri alert semantics dan nama tombol dismiss.

**Regresi:** speech permission/unsupported + stale onend/onresult sesudah cleanup; rejected list/load/dialog/search; assert pesan error dan accessible alert/nama kontrol.

## Hal yang sudah membatasi risiko (bukan temuan baru)

- `agent-event` memfilter **runId dan sessionId** (`useAgent:375-377`), sehingga event run lama yang tiba setelah switch normal tidak langsung meracuni sesi baru. Race yang dilaporkan ada di continuation await, queue dispatch, cwd global dan buffer tanpa identity; bukan ketiadaan filter umum.
- Save frontend diserialisasi `persistenceQueueRef`; backend juga memiliki `SESSION_IO_LOCK`. Keduanya tidak menjamin revision correctness untuk semua callback/operasi di luar queue.
- Navigation generation mengabaikan respons load lama setelah switch/new normal (`979`), dan compaction mengecek generation+session (`1309`). Clear, source-side send continuations, workspace setters dan queue belum mengikuti aturan ini.
- Review result memakai key `[session,run]` dan bounded cache 256; backend merge selfImprovement menjaga review native dari stale frontend save. Bukan menemukan review cache tanpa batas.
- Hook viewport melepaskan listener media query; hook modal melepaskan keydown/frame. Tidak ditemukan conditional hook di App atau empat file hooks yang dibaca.
- TypeScript/Rust message mapping yang diperiksa mempertahankan preamble/segments/council/run identity/review. `notice` adalah transient UI dan tidak dipersist Rust; audit ini tidak mengklaim semua metadata hilang lewat Serde.

## Coverage test yang ada dan gap bernilai tinggi

Test **dibaca, tidak dijalankan**:
- `useAgent.reflection.test.tsx` mencakup routing review/reflect, save-before-review, review delayed origin, failed/cancelled review guard, review selama stale load, notification tanpa echo prompt, dan **cancel-on-switch**. Mock listener memakai satu Map entry per nama dengan registrasi segera, sehingga tidak menguji beberapa listener/late cleanup/registration failure. Save/send mock umumnya resolve segera, sehingga startup send/nav race tidak tervalidasi.
- `useSpeechRecognition.test.tsx` mencakup final-only transcript dan onend, bukan unsupported/permission/cleanup/lang change.
- `App.test.tsx` memock seluruh useAgent; baik untuk command/panel/props wiring, tidak dapat membuktikan persistence/IPC ordering. Viewport tests memilih ukuran awal, bukan respons request asynchronous.
- `ChatCanvas.commands.test.tsx` mempunyai queue display/edit/remove dan auto-dispatch normal, serta execution mode pada **direct send**. Queue dispatcher/resume mengirim hanya `(prompt,attachments)` (`ChatCanvas:379,390`), tidak menyimpan council/executionMode (`1162-1169`): queued prompt tidak mempertahankan mode direct send. Untuk `request_review` yang dipilih di composer, queued follow-up bisa jatuh ke `settings.executionMode ?? 'turbo'` (`useAgent:789`). Ini perlu ditambahkan sebagai regression acceptance/security-policy check; kontrak mode terperinci perlu diselaraskan dengan audit backend.

Prioritas regresi setelah persetujuan implementasi:
1. Deferred save/send/abort registration dengan navigation, old catch vs new run, dan cwd identity (ST-01/02/05).
2. Queue tidak dispatch saat New/Switch/Clear; entry membawa session + execution/council options, tidak dibuang jika send ditolak.
3. Edit/undo/retry tidak melanjutkan ke session baru; hold RAF saat retry/Clear (ST-03/11).
4. Clear/Delete saat compact/undo/load; revision/tombstone + metadata mutation ordering (ST-04/07/14).
5. Settings concurrent refresh/save rollback (ST-08); reconnect reset representation + council early winner (ST-09/10).
6. Listener readiness/rejection/StrictMode cleanup dan Memory workspace/modal shortcut boundary (ST-12/13/15).

## Keterbatasan pemeriksaan

Sebagian tool `read_file/search_files` sempat timeout; pembacaan yang diperlukan dilanjutkan lewat Python `Path.read_text` + nomor baris pada file sumber yang sama, tanpa menjalankan kode aplikasi. Satu command baca menghasilkan output lalu timeout exit 124; pembacaan susulan berhasil exit 0. Tidak ada temuan yang didasarkan pada hasil test/build palsu. Tidak ada severity C yang ditetapkan tanpa bukti yang cukup.

Pemeriksaan akhir worktree mendeteksi perubahan bersamaan pada `package.json`, `src-tauri/src/agent/provider.rs`, dan `vitest.config.ts`; subagent audit ini tidak menulis ketiganya dan tidak membatalkannya. Satu-satunya file yang dibuat/diubah subagent ini adalah laporan ini. Verifikasi struktur laporan lewat Python menemukan 15 ID unik: H=6, M=8, L=1; cakupan file utama dan desain live-run routing/reset-attempt tercantum. Ini verifikasi dokumen, bukan eksekusi test aplikasi.

Evaluasi internal singkat: akurasi 4/5 (call path silang frontend/native sudah dicocokkan, belum runtime); kelengkapan 4/5 (seluruh scope hooks/types/App/main dibaca, components/native hanya jalur pendukung); kejelasan 4/5 (temuan diberi lokasi/trigger/dampak/fix/test, beberapa race saling terkait); actionability 4/5 (skenario deferred promise/RAF spesifik, belum test executable sesuai larangan); keringkasan 4/5 (laporan panjang untuk menjaga actionable evidence). Peningkatan utama: jalankan regresi interleaving setelah tahap read-only disetujui selesai; bukan menambah klaim verifikasi di tahap ini.
