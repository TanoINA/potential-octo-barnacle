# Audit layer UI AetherAgent — komponen, CSS/UX, dan celah pengujian

## Batas audit dan verifikasi

- **Audit source-only / source-derived layout; belum UI live.** Tidak ada pengukuran browser/WebView, screenshot, build, lint, atau test yang dijalankan. Skenario regresi di bawah merupakan rekomendasi, bukan hasil eksekusi.
- Membaca seluruh **26 komponen produksi `src/components/*.tsx` (9.218 baris)**, seluruh **18 file test komponen**, `src/index.css`, dan satu utilitas `src/utils/audioChime.ts`. Membaca `App.tsx`, `App.test.tsx`, hook aksesibilitas/viewport/speech, serta bagian `useAgent.ts` dan Rust yang diperlukan untuk memverifikasi wiring aktual.
- Basis: React 19, react-markdown 10, TanStack React Virtual 3 sesuai manifest; minimum jendela native 900 × 600 (`src-tauri/tauri.conf.json:18-19`). Tidak ditemukan `AGENTS.md` pada root/ancestor yang diperiksa maupun subtree `src`/`docs`.
- Hanya file laporan ini ditulis. Tidak mengubah source/test/config, menjalankan command UI/backend, atau menyentuh `docs/audit-evidence-tools-2026-11-08.md`.
- Worktree awal: file bukti tersebut sudah untracked. Pada pemeriksaan akhir muncul perubahan `package.json`, `vitest.config.ts`, dan direktori `docs/audit/` dari pekerjaan paralel; perubahan tersebut **bukan pekerjaan auditor UI ini** dan tidak disentuh.
- Sebagian panggilan pembacaan sempat timeout; pembacaan ulang berhasil. Tidak ada hasil test/build yang diasumsikan dari test source atau keberadaan script.

Severity: **P1** = risiko kehilangan data, kontrol keselamatan menyesatkan, atau fungsi utama rusak; **P2** = salah perilaku/aksesibilitas penting; **P3** = masalah UX/performa terbatas. Semua temuan berikut berasal dari penelusuran source, bukan reproduksi live.

## Temuan prioritas

### UI-01 — P1 — “Request Review” menjanjikan approval yang tidak ditegakkan runtime

**Lokasi:** `src/components/ChatCanvas.tsx:55-59,509-514,1215-1219`; `src/hooks/useAgent.ts:789,866`; `src-tauri/src/agent/runner.rs:800-806,1067-1126,1190-1220`; `src-tauri/src/agent/tools.rs:36-41`.

- **Pemicu:** pilih Request Review, lalu model mengeluarkan `write_file`, `run_command`, MCP call, atau subagent tanpa bertanya.
- **Bukti/wiring:** mode benar-benar dikirim ke request, tetapi penggunaan mode di runner hanya menambahkan instruksi system prompt. Dispatch tool mengecek cancellation, lalu mengeksekusi tool; `ToolContext` tidak membawa mode/approval token. Jalur subagent juga tidak menerima mode tersebut. Tidak ada dialog approval atau event approval di komponen.
- **Dampak:** teks “Every command requires approval” bukan jaminan. Operator dapat mengandalkan kontrol yang sebenarnya hanya nasihat kepada model.
- **Perbaikan minimum:** sampai gate native tersedia, ubah disclosure menjadi “instruksi kepada model, bukan enforcement” dan jangan menjanjikan approval. Perbaikan keselamatan sebenarnya harus menahan eksekusi native hingga persetujuan eksplisit, termasuk MCP/subagent.
- **Regresi:** provider tiruan mengeluarkan write/execute langsung dalam Request Review; pastikan tidak ada side effect sebelum approval, penolakan tidak mengeksekusi apa pun, dan subagent/MCP mengikuti kebijakan yang sama. Test UI sekarang hanya memastikan label dan payload mode, bukan enforcement (`ChatCanvas.commands.test.tsx:207-234`).

### UI-02 — P1 — `/plan` dikonsumsi UI tanpa pernah dikirim

**Lokasi:** `src/components/ChatCanvas.tsx:149,1016-1027,1031-1143,1185-1189`; implementasi tujuan `src/hooks/useAgent.ts:748-752`.

- **Pemicu:** pilih `/plan`, isi tujuan, lalu Send/Enter.
- **Bukti/wiring:** `/plan` masuk `allSlashItems`, sehingga `send()` masuk cabang `executeSlashCommand`. Switch tidak memiliki `case '/plan'`; default tidak melakukan apa pun. Setelah itu composer dan attachment dibersihkan. Transformasi prompt `/plan` di `useAgent.sendMessage` tidak pernah tercapai dari alur ini.
- **Dampak:** fitur rencana tidak bekerja, prompt dan attachment pengguna hilang dari composer tanpa run.
- **Perbaikan minimum:** route `/plan` ke `onSendMessage(draft, attachments, options)` dan bersihkan composer hanya jika accepted; atau jangan intercept `/plan` sebagai command lokal.
- **Regresi:** kirim `/plan buat parser` lewat Enter dan tombol Send, dengan attachment; assert satu request berisi command/tujuan dan attachment. Tambahkan kasus reject agar draft tetap tersimpan.

### UI-03 — P1 — Aksi Git pada kartu pesan lama bekerja pada workspace aktif, bukan workspace pembuat pesan

**Lokasi:** `src/components/FilesChangedCard.tsx:60-82,106-118,120-133,269-297`; `src/components/MessageRow.tsx:365-369`; `src/App.tsx:106-108`; `src/hooks/useAgent.ts:1180-1183`; `src-tauri/src/lib.rs:460-497`; `src-tauri/src/agent/git.rs:184-218`.

- **Pemicu:** sesi berisi kartu perubahan `src/config.ts` dari workspace A; ganti workspace menjadi B; klik Revert/Accept pada kartu lama.
- **Bukti/wiring:** kartu hanya menerima toolCalls dan artifact callback, bukan workspace/snapshot identity. Invoke mengirim `{path}` saja; command native mengambil `workspace_cwd` saat itu. `gitStatus` kartu hanya dimuat saat mount sehingga label branch bisa tetap A sementara aksi menuju B.
- **Dampak:** file relatif bernama sama di B bisa di-stage/discard; untuk untracked file, Revert dapat menghapus file. Revert tidak meminta konfirmasi dan tetap tersedia ketika agent sedang berjalan. Ini juga bukan undo perubahan satu turn: backend checkout/restore bekerja terhadap state Git saat ini.
- **Perbaikan minimum:** ikat aksi ke workspace asal dan revision/snapshot yang jelas, nonaktifkan kartu saat workspace tidak cocok/agent aktif, serta tampilkan konfirmasi scope discard. Untuk kartu historis yang tidak punya provenance, cukup sediakan Review tanpa mutation.
- **Regresi:** dua repo dengan path sama dan konten berbeda; pindah workspace lalu coba aksi kartu A. Assert B tidak tersentuh. Tambahkan untracked file, perubahan manual setelah turn, agent aktif, serta refresh branch setelah workspace berubah.

### UI-04 — P1 — “Edit & Resend from Here” pada context menu langsung memotong riwayat tanpa editor

**Lokasi:** `src/components/ContextMenu.tsx:565-573`; `src/App.tsx:288`; `src/hooks/useAgent.ts:1437-1498`.

- **Pemicu:** klik kanan pesan user lama → Edit & Resend from Here.
- **Bukti/wiring:** menu langsung memanggil `onEditAndResendMessage(id, foundMessage.content)` dengan teks lama, bukan membuka editor. Hook menghentikan run aktif, memangkas pesan/artifact mulai titik tersebut, menyimpan, lalu mengirim ulang.
- **Dampak:** aksi yang terlihat sebagai membuka pengeditan justru destruktif dan dapat memicu request/tool baru sebelum pengguna mengubah teks. Berbeda dari tombol Edit inline yang memang membuka textarea.
- **Perbaikan minimum:** dispatch permintaan membuka editor untuk message ID, tanpa mutation hingga Save & Submit; atau ganti nama menjadi Retry from here dengan konfirmasi pemangkasan. Jangan menerima action ini saat active run tanpa penjelasan eksplisit.
- **Regresi:** context menu Edit harus menghasilkan editor berisi teks asli dan nol pemanggilan send/persist/abort sampai submit; Cancel mempertahankan seluruh transcript.

### UI-05 — P1 — Prompt antrean melewati dispatcher dan kehilangan execution mode/Council

**Lokasi:** `src/components/ChatCanvas.tsx:290-294,367-390,1146-1219`; fallback `src/hooks/useAgent.ts:789`; test yang mengunci perilaku lama `src/components/ChatCanvas.commands.test.tsx:155-204`.

- **Pemicu:** pilih Request Review atau Council, lalu Queue saat run aktif; atau antrekan `/new`, `/clear`, `/model`, `/undo`.
- **Bukti/wiring:** queue menyimpan hanya prompt/attachment; auto-dispatch dan Resume langsung memanggil `onSendMessage(prompt, attachments)` tanpa options dan tanpa `executeSlashCommand`. Mode beralih ke fallback settings/Turbo; Council menjadi run tunggal. Command lokal dikirim sebagai prompt model (kecuali alias yang memang ditangani hook).
- **Dampak:** queued run tidak sesuai kontrol yang terlihat; khusus Request Review, perilaku bisa berubah ke Turbo. Command UI dapat ditafsirkan model sebagai instruksi biasa.
- **Perbaikan minimum:** satu dispatcher untuk normal send dan queue. Simpan snapshot opsi run dalam queued item, atau nyatakan eksplisit bahwa opsi diambil saat dispatch; command lokal harus dieksekusi sebagai command lokal, bukan request model.
- **Regresi:** queue Request Review, Council A/B, attachment, `/new`, `/clear`; periksa payload/jalur aktual. Ubah assertion dua argumen pada test queue agar memeriksa opsi keselamatan, bukan justru mengabadikan kehilangan opsi.

### UI-06 — P2 — Queue menghapus item sebelum request diterima dan dapat macet ketika review berlangsung

**Lokasi:** `src/components/ChatCanvas.tsx:368-390,1767-1774`; penolakan `src/hooks/useAgent.ts:707-710`.

- **Pemicu:** queued item di-dispatch ketika konfigurasi tidak valid/session operation aktif sehingga `onSendMessage` mengembalikan false; atau streaming selesai saat `isReviewing=true`.
- **Bukti:** item langsung di-slice sebelum await dan boolean accepted tidak dibaca. Kondisi auto-dispatch hanya menangkap transisi true→false streaming; jika saat itu reviewing, transisi ditandai selesai. Saat review berakhir tidak ada transisi baru dan `stoppedWithQueue` tetap false sehingga Resume tidak muncul.
- **Dampak:** prompt/attachment antrean hilang ketika ditolak, atau tersisa tanpa cara melanjutkan melalui Resume.
- **Perbaikan minimum:** dispatch async dengan guard in-flight; dequeue hanya setelah accepted; evaluasi kesiapan queue berdasarkan idle/review/session operation, bukan hanya edge streaming. Tampilkan Resume untuk queue idle yang belum dikirim.
- **Regresi:** rejected/throwing sender mempertahankan item, dua event tidak menduplikasi dispatch, review berakhir melanjutkan/menampilkan Resume, Stop mempertahankan queue.

### UI-07 — P1 — Shortcut global menembus modal; memory editor dapat menulis konten A ke workspace B

**Lokasi:** `src/App.tsx:45-71,227-235`; `src/hooks/useModalAccessibility.ts:37-61`; `src/components/MemoryModal.tsx:30-50,65-71`; `src/hooks/useAgent.ts:1551-1556`.

- **Pemicu:** buka Memory/Settings/Model Picker, lalu Ctrl+N/Ctrl+K/Ctrl+,; untuk memory, buka workspace A, ubah instruksi, Ctrl+N lalu pilih workspace B dari sidebar di luar modal.
- **Bukti/wiring:** hook modal hanya menangkap Escape/Tab. Shortcut App tetap memanggil newSession/membuka modal lain; backdrop tidak membuat underlying UI inert. Memory read effect bergantung `open,onReadMemory`, sementara callback read stabil dan tidak bergantung workspace. Save hanya mengirim content kepada command yang memakai workspace aktif.
- **Dampak:** sesi/run dapat di-reset ketika pengguna sedang dalam dialog; modal bertumpuk memasang beberapa trap aktif. Memory dapat tetap menampilkan draft A setelah pergantian workspace dan menyimpan ke B.
- **Perbaikan minimum:** guard shortcut App selama modal aktif; satu modal manager/topmost trap dan underlying UI inert. Bind memory ke workspace ID/path saat open; reload/konfirmasi discard pada perubahan target, dan validasi workspace yang sama ketika save.
- **Regresi:** Ctrl+N di setiap modal tidak mengubah sesi; Ctrl+K tidak membuat modal kedua tanpa lifecycle terkelola. Memory A→B tidak menampilkan/menyimpan draft A ke B. Uji fokus/Tab/Escape pada nested inspector dan fokus kembali ke trigger.

### UI-08 — P2 — Paste dari context menu tidak memperbarui state input React terkontrol

**Lokasi:** `src/components/ContextMenu.tsx:353-388`; composer terkontrol `src/components/ChatCanvas.tsx:1854-1861`.

- **Pemicu:** klik kanan composer/settings input → Paste atau Paste as Code Block, kemudian Send atau ketik lagi.
- **Bukti:** handler menetapkan `inputEl.value` langsung lalu dispatch Event('input'). Pada input terkontrol React, assignment property melalui setter React memperbarui value tracker sebelum event sehingga perubahan ini dapat tidak dianggap perubahan state. Nilai DOM dan state composer tidak sinkron; tidak ada penggunaan native prototype setter atau callback state.
- **Dampak:** teks tampak ter-paste tetapi Send membaca draft lama; render berikutnya mengembalikan nilai lama. Context menu global juga menawarkan “Clear Composer” ketika klik kanan input selain composer.
- **Perbaikan minimum:** pertahankan native context menu untuk input, atau gunakan handler terkontrol yang memutasi state/callback target secara benar. Jangan menawarkan aksi composer pada setiap input termasuk field rahasia/pengaturan.
- **Regresi:** render App/ChatCanvas asli, paste lewat menu lalu Send; assert request sama dengan teks DOM. Ulangi pada settings dan seleksi substring, bukan textarea uncontrolled tiruan.

### UI-09 — P2 — Execution trace menyembunyikan error/result non-terminal dan mengumumkan keberhasilan

**Lokasi:** `src/components/ExecutionTrace.tsx:70-77,130-209,229-271,338-402`.

- **Pemicu:** read/write/use_skill/fetch/MCP/spawn_subagent berakhir `status='error'` atau tanpa artifact.
- **Bukti:** semua status selain running/pending dipetakan label lampau seperti “Loaded skill”/“Wrote”. `hasError` hanya membuat accordion terbuka; tidak ada badge error per-row/summary dan body result hanya dirender untuk `run_command`/`manage_task`. Result/args tool non-terminal tanpa artifact tidak bisa diperiksa dari trace. Row clickable adalah div tanpa keyboard handler/tabIndex.
- **Dampak:** pengguna melihat narasi seolah aksi berhasil, tetapi alasan gagal dan bukti panggilan hilang. Katalog Tools Inspector adalah katalog, bukan pengganti receipt eksekusi.
- **Perbaikan minimum:** status sukses/error/cancelled eksplisit; expandable detail semua tool berisi arg/result/error terbatasi, dengan renderer text inert; gunakan button untuk row interaktif dan label aksesibel.
- **Regresi:** success/error/cancelled untuk native/MCP/skill/subagent, hasil pendek tanpa artifact, malformed arguments; operator keyboard dapat membuka detail dan membaca error.

### UI-10 — P2 — Bubble “task inspector” tidak membuka inspector ketika belum ada artifact

**Lokasi:** `src/components/ChatCanvas.tsx:1350-1369,1653-1662`; `src/App.tsx:131-132,164-171`; `src/components/ArtifactPane.tsx:94-99`.

- **Pemicu:** ada subagent running, artifactCount=0, klik bubble Active.
- **Bukti/wiring:** tombol enabled karena runningTasks>0 dan mengubah artifactOpen, tetapi App hanya merender pane jika artifacts.length>0. Pane sendiri berisi Code/Diff/Tool Output/Shell, bukan daftar task. Count bubble juga mengabaikan backgroundTasks.
- **Dampak:** tombol “click to inspect” tidak menghasilkan inspector; background process aktif tidak tercermin pada bubble. Detail yang sebenarnya ada di composer harus dibuka melalui kartu lain.
- **Perbaikan minimum:** arahkan bubble ke drawer task nyata yang menerima subagents/backgroundTasks, tanpa syarat artifact; atau arahkan/expand kartu task yang sudah ada dan ubah label menjadi artifacts-only bila memang demikian.
- **Regresi:** App dengan hanya subagent running dan tanpa artifact; klik membuka detail task. Ulangi dengan hanya background task dan task gagal.

### UI-11 — P2 — Artifact baru dapat mengusir pilihan Live Shell

**Lokasi:** `src/components/ArtifactPane.tsx:42-50,76-80`; `src/hooks/useAgent.ts:472-479`; `src/components/ArtifactPane.test.tsx:15-25`.

- **Pemicu:** pane terbuka pada Live Shell lalu agent memproduksi artifact baru.
- **Bukti/wiring:** setiap artifact event memanggil setActiveArtifactId(artifact.id). Pane menganggap setiap perubahan ID sebagai seleksi pengguna dan setTab sesuai tipe artifact, sehingga InteractiveTerminal unmount. Test yang ada hanya mengubah content artifact dengan ID tetap, bukan kedatangan artifact baru.
- **Dampak:** sesi shell/input/history lokal hilang; command shell yang sedang berjalan tidak punya lifecycle cancellation pada unmount (UI-nya hilang, invoke tetap berjalan).
- **Perbaikan minimum:** bedakan active selection eksplisit dari artifact arrival; saat user memilih shell jangan auto-switch. Pertahankan state shell di parent atau tetapkan lifecycle close yang jelas.
- **Regresi:** pilih shell, tulis draft/jalankan command pending, emit artifact dengan ID baru; tab/draft/history tetap. Close saat pending harus menyatakan apakah command diteruskan atau dibatalkan.

### UI-12 — P2 — Layout minimum native dengan Live Shell menyisakan chat 260 px

**Lokasi:** `src/App.tsx:84,116,246-257`; `src/components/Sidebar.tsx:301`; `src/hooks/useIsNarrowViewport.ts:3`; `src/components/ChatCanvas.tsx:1309-1345,1873-1875,1957-1983,2090-2210`; `src/index.css:15`; `src-tauri/tauri.conf.json:18-19`.

- **Pemicu:** window 900 × 600, sidebar terbuka, Live Shell dibuka; gunakan nama model panjang, Request Review, atau Queue+Stop.
- **Bukti/source-derived layout:** sidebar w-64=256 px, shell w-full max-w-sm shrink-0 hingga 384 px: 900−256−384=260 px untuk chat. Breakpoint overlay memakai viewport 767 px, bukan sisa lebar chat. Pada viewport 900, kontrol `sm`/`md` tetap versi lebar. Toolbar/composer tidak wrap; root/body menutup overflow.
- **Dampak yang perlu divalidasi live:** tombol send/stop/mode dan toolbar berpotensi keluar dari area chat atau berdesakan; native minimum tidak mencegahnya.
- **Perbaikan minimum:** pada lebar gabungan pane yang tidak cukup, jadikan shell drawer/overlay atau collapse sidebar; gunakan container query/row wrap untuk composer. Jangan menyelesaikan dengan hanya mengecilkan font.
- **Regresi:** ukur rect seluruh kontrol pada 900×600 dan 1024×768, sidebar+shell+Council+queue aktif, nama model terpanjang, skala 125/150%. Assert tidak ada elemen penting di luar viewport/tertutup.

### UI-13 — P2 — Kartu task di composer dapat memakan tinggi jendela dan menyembunyikan kontrol

**Lokasi:** `src/components/ChatCanvas.tsx:1589,1653-1662,1757-1817,1821-1846`; `src/components/SubagentTreeCard.tsx:52`; `src/components/BackgroundTasksCard.tsx:83`; `src/index.css:15`.

- **Pemicu:** native height 600, buka kedua kartu task dengan daftar panjang; tambah antrean, attachment, atau Council banner.
- **Bukti/source-derived layout:** keseluruhan composer `shrink-0` tidak memiliki max-height/scroll. Masing-masing body kartu boleh 45vh dan 50vh; pada 600 px total body saja 570 px, belum header kartu, composer, chat header, queue dan banner. Queue/attachment juga tidak dibatasi tinggi.
- **Dampak yang perlu divalidasi live:** composer bisa melampaui viewport; chat kehilangan area dan Send/Stop tidak terjangkau karena root overflow hidden. Membatasi setiap kartu sendiri bukan batas untuk total composer.
- **Perbaikan minimum:** taruh task inspector dalam drawer terpisah atau satu area composer dengan total max-height dan overflow; queue/attachment mempunyai area scroll/collapse. Pertahankan composer inti/Stop tetap terlihat.
- **Regresi:** 600 px height dengan task panjang di kedua kartu, banyak queue item/attachment dan Council; fokus keyboard tetap dapat mencapai Stop/Send tanpa elemen tertutup.

### UI-14 — P2 — Context menu panjang tidak memiliki scroll; event keyboard dapat sekaligus mengirim composer

**Lokasi:** `src/components/ContextMenu.tsx:733-735,796-892,896-903,913-930,958-962,1033-1037`; `src/components/ChatCanvas.tsx:1242-1268`.

- **Pemicu:** klik kanan kode di pesan dengan seleksi teks sehingga banyak section bergabung; gunakan ArrowDown/Enter ketika fokus sebelumnya masih di composer.
- **Bukti/source-derived layout:** clamp posisi tidak bisa membuat menu yang lebih tinggi daripada viewport muat; menu/submenu tidak memiliki max-height/overflow-y. Listener global scroll menutup menu. Keyboard navigation memakai focusedIndex visual tanpa memindahkan fokus DOM, dan handler berada pada window bubble; handler textarea dapat lebih dahulu menerima Enter/Arrow.
- **Dampak:** item bawah tidak terjangkau pada jendela pendek; Enter dapat memicu send composer selain action context menu. Tidak ada role menu/menuitem/active descendant atau pemulihan fokus target.
- **Perbaikan minimum:** menu bounded-height, scroll internal tidak memicu close, focus/menu semantics yang benar, dan handler keyboard scoped/capture yang menghentikan event sebelum underlying editor. Clamp ukuran aktual submenu untuk hover, ArrowRight, dan Enter dengan aturan sama.
- **Regresi:** menu paling panjang pada 900×600, akses item terakhir dengan mouse dan keyboard; Enter menjalankan tepat satu aksi menu dan tidak mengirim draft composer. Test saat ini mengirim key ke window, sehingga melewati risiko event textarea.

### UI-15 — P2 — Split diff menghilangkan baris sah yang diawali dua tanda minus/plus

**Lokasi:** `src/components/SideBySideDiff.tsx:90-93,110-128`.

- **Pemicu:** diff memuat penghapusan baris source `--flag` (baris patch `---flag`) atau penambahan source `++counter` (baris patch `+++counter`).
- **Bukti:** parser mengabaikan semua rawLine.startsWith('---')/('+++') sebagai header file, bahkan saat berada di hunk. Metadata `diff --git`, `index`, `\\ No newline at end of file`, dan trailing empty line juga masuk cabang unchanged dan dapat menambah nomor baris palsu.
- **Dampak:** review split dapat menyembunyikan perubahan nyata dan menampilkan line number salah; ini bukan sekadar warna/highlight berbeda.
- **Perbaikan minimum:** parse state header/hunk sesuai unified diff; header file hanya di posisi header, metadata tidak menambah counter, dan hanya context/deletion/addition dalam hunk yang menjadi source row.
- **Regresi:** patch dengan `---flag`/`+++counter`, beberapa hunk/file, no-newline marker, empty file, dan konteks trailing newline. Cocokkan konten dan nomor baris terhadap unified patch. Belum ada direct test SideBySideDiff.

### UI-16 — P2 — Auto-tail TerminalOutput berhenti saat chunk baru lebih tinggi dari threshold

**Lokasi:** `src/components/TerminalOutput.tsx:26-41`; penggunaan `src/components/ExecutionTrace.tsx:394-402`.

- **Pemicu:** pengguna berada di bawah log lalu update menambah beberapa baris sekaligus lebih dari 24 px.
- **Bukti:** effect mengukur jarak ke bawah sesudah output baru dirender; tinggi baru membuat jarak melebihi threshold walaupun sebelum update pengguna berada di bawah. Komponen tidak menyimpan stickiness dari event scroll sebelum mutation.
- **Dampak:** output berhenti mengikuti log justru ketika output tumbuh; pengguna harus scroll manual. Ini terpisah dari chat virtualizer yang sudah mempunyai stickToBottomRef.
- **Perbaikan minimum:** rekam status near-bottom di onScroll sebelum update lalu tail berdasarkan ref itu; jangan mengambil keputusan dari tinggi DOM yang sudah berubah.
- **Regresi:** initial bottom + append chunk besar tetap tail, deliberate scroll-up tidak ditarik, kembali ke bottom mengaktifkan tail. Verifikasi memakai elemen dengan dimensi nyata/browser, bukan jsdom default 0.

### UI-17 — P2 — Error library yang sudah ditangani modal ditelan adapter App

**Lokasi:** `src/components/ArtifactsLibraryModal.tsx:75-93,247-260`; `src/App.tsx:190-196`; `src/hooks/useAgent.ts:1211-1217`; test `src/components/ArtifactsLibraryModal.test.tsx:113-133`.

- **Pemicu:** native `list_all_artifacts` gagal dalam App asli.
- **Bukti/wiring:** komponen memiliki catch+Retry, tetapi callback nyata `listAllArtifacts` menangkap error dan me-return [] sukses. Modal menampilkan “No artifacts have been generated yet”; error toast global memang ada tetapi local Retry tidak tersedia. Test modal memasang callback rejected langsung, tidak meniru adapter App.
- **Dampak:** kegagalan dibedakan salah sebagai library kosong dan jalur pemulihan yang sudah dibuat tidak tercapai.
- **Perbaikan minimum:** setelah setLastError, rethrow error atau return Result eksplisit yang ditangani modal; jangan mengubah kegagalan menjadi array kosong sukses.
- **Regresi:** render App dengan invoke list_all_artifacts reject, buka library dan assert error lokal+Retry; retry sukses menampilkan item, bukan empty state.

### UI-18 — P2 — Error awal pengiriman dapat hilang di bubble karena empty segments mengalahkan content

**Lokasi:** `src/hooks/useAgent.ts:722-730,833-839,871-877`; `src/components/MessageRow.tsx:158-185,264-270,273-275`.

- **Pemicu:** persist/send_message gagal sebelum provider menghasilkan teks.
- **Bukti/wiring:** assistant dibuat dengan `segments=[{text:'',toolCalls:[]}]`. Catch mengisi `message.content='Error: ...'` dan mematikan streaming, tetapi tidak mengisi segment. Renderer melihat segments.length>0, lalu setiap segment kosong dilewati; cabang content fallback hanya berlaku jika segments tidak ada/kosong.
- **Dampak:** transcript punya bubble assistant kosong meskipun error tersimpan di content. Toast global dapat ditutup sehingga tidak ada pesan error yang bisa dibaca di turn. Pada load, segments sudah ada sehingga migrasi tidak otomatis memulihkan body.
- **Perbaikan minimum:** pilih jalur segment hanya jika ada segment yang benar-benar renderable, atau perbarui segment/error field yang dirender saat start gagal.
- **Regresi:** invoke send_message reject sebelum chunk, persist reject, dan load ulang turn tersebut; teks error tampak di transcript, tidak hanya toast.

## Catatan tambahan dan batas kepastian

- **Markdown/hooks:** `MarkdownRenderer.tsx:165-171` menempatkan return khusus `plan` sebelum useEffect. Ini melanggar susunan hook bila instance CodeBlock yang sama berganti bahasa. Namun mapping renderer `components` dibuat ulang di setiap render (`MarkdownRenderer.tsx:34-152`), yang saat ini dapat me-remount subtree; karena itu **tidak diklaim sebagai crash yang sudah reachable/terbukti**. Pindahkan hook sebelum return sebagai perbaikan kecil, dan uji transisi fence parsial→plan→kode biasa. Renderer function yang dibuat ulang juga berpotensi mereset copied/plan state saat content streaming berubah; uji identity/lifecycle sebelum optimasi.
- **Preservasi kode:** sanitasi emoji (`MarkdownRenderer.tsx:12-15`) hanya melindungi fence tiga backtick tertutup dan inline satu backtick. Fence tilde, fence empat backtick dengan tiga backtick di dalamnya, indent code, serta fence streaming yang belum ditutup perlu fixture byte-exact; jangan menyatakan semua format kode sudah aman hanya dari dua fixture triple-backtick.
- **Virtualizer chat:** item key message ID dan measureElement sudah dipakai; tidak ditemukan bukti cukup untuk menuduh overlap row secara umum. Callback retry/edit/rollback baru pada setiap visible row (`ChatCanvas.tsx:1543-1549`) membatalkan manfaat memo MessageRow saat streaming; ini kandidat performa **P3**, bukan crash. Efek reset scroll juga bergantung sessionTitle (`ChatCanvas.tsx:778-792`), sehingga rename sesi dapat menarik pembaca kembali ke bawah; uji rename saat scroll-up.
- **CSS/aksesibilitas:** `index.css:59-60` menonaktifkan outline semua input/textarea, termasuk radio/checkbox yang tidak selalu punya pengganti fokus pada wrapper (misalnya ModelPicker effort). Scope suppression ke text input yang benar-benar memiliki indikator focus/focus-within, jangan global. Tombol/card clickable div untuk plan, timeline tick/popover, dan trace tidak semuanya keyboard-accessible. Hook modal memerlukan pengujian browser karena visibility memakai getClientRects, sementara jsdom tidak menyediakan layout nyata.
- **Speech:** hook menyediakan isSupported/error, tetapi ChatCanvas hanya mengambil isListening/toggleListening (`ChatCanvas.tsx:840`); mikrofon dapat tampak enabled tetapi tidak melakukan apa pun saat API tidak tersedia/permission ditolak. Tampilkan status error dan disable unsupported, lalu uji lewat komponen, bukan hook saja.
- **Clipboard/lifecycle:** BackgroundTasksCard dan ArtifactsLibraryModal langsung mengubah label menjadi Copied tanpa await/catch; MessageRow tidak menangkap rejection. Beberapa copy timers (MessageRow, ContextMenu, TimelineScrubber, TerminalOutput, InteractiveTerminal, library/tasks) tidak dibersihkan. Prioritaskan akurasi feedback/penanganan rejection; jangan membesar-besarkan setState setelah unmount menjadi memory leak berat tanpa bukti.
- **Native drop listener:** `ChatCanvas.tsx:681-718` melakukan async register dan cleanup hanya jika unlisten sudah tersedia. Jika unmount/StrictMode cleanup mendahului resolusi, listener baru tertinggal. Gunakan disposed flag dan segera unlisten bila registrasi selesai setelah dispose. Test setup→cleanup→resolve, kemudian pastikan tidak ada listener hidup ganda.
- **Util audio/notification:** util dibaca penuh; tidak ada defect util berdampak tinggi yang cukup terkonfirmasi untuk dijadikan finding mandiri. Call-site done (`useAgent.ts:565-579`) mengirim Task Complete/chime sebelum memeriksa cancelled/failed; bedakan notifikasi selesai, gagal, dan dibatalkan agar tidak memberi kesan sukses. Dukungan Notification API di WebView belum diuji.

## Coverage komponen dan test gaps

Inventaris berikut mencakup seluruh komponen produksi; keberadaan test **bukan** klaim test lulus. Tidak ada angka coverage runtime yang dihasilkan.

| Komponen | Test langsung yang dibaca | Fokus audit / gap utama |
|---|---|---|
| AetherLogo | Tidak ada | SVG dekoratif; tidak ada bug fungsional mandiri yang cukup terkonfirmasi |
| ArtifactPane | Ada | Test shell content-update saja; hilang kasus artifact ID baru, user selection, tail, ukuran row |
| ArtifactsLibraryModal | Ada | List/filter/open chat/error; gap adapter App, retry race/cancellation, clipboard gagal, layout split |
| BackgroundTasksCard | Ada | Expand/Kill/log happy path; gap keyboard, failure status detail, copy reject, total tinggi composer |
| ChatCanvas | 3 file | Commands/reflection/drop-paste; gap `/plan`, queue options/reject/review, native listener cleanup, scroll riil |
| ContextMenu | Ada | Target/selection/file/code/navigation; gap controlled paste, editor action, event propagation, menu tinggi |
| CouncilView | Tidak ada | Selection handler nyata dipetakan; gap memilih candidate yang masih streaming/failed, respons panjang, copy answer terpilih |
| ErrorBoundary | Tidak ada | Wrapper root terverifikasi; gap fallback setelah error render dan pemulihan reload; bukan pelindung error async handler |
| ExecutionTrace | Tidak ada | UI-09; perlu fixture status/receipt semua tool dan keyboard detail |
| FilesChangedCard | Ada | Test hanya ringkasan/minimize; belum mutation, scope workspace, failed revert/commit/push, changes-after-stage |
| InteractivePlanCard | Ada | Parse/toggle single mount; belum rerender rawPlan, streaming/remount, keyboard dan semantik checkbox |
| InteractiveTerminal | Tidak ada | Submit IPC/error/histori; gap unmount pending, workspace switch, cancellation UX, output panjang, history reset |
| MarkdownRenderer | Ada | Indent triple fence/link; gap semua format fence, streamed fence, lang transition, URL encoding dan byte-exact copy |
| MemoryModal | Tidak ada | Dirty-close/read/save ditelusuri; gap workspace A→B, save pending, unsaved discard, fokus dan error |
| MessageRow | Ada | Action/segments/notice happy path; gap startup error empty segments, clipboard reject, virtualized remount editor |
| ModelPicker | Tidak ada | Selection/staging/override ditelusuri; gap Cancel setelah thinking/reset, short-height popover clipping, keyboard listbox |
| SelfImprovementBadge | Ada | Banyak test scope/read stale/unmount/Escape; gap integrasi portal+virtualizer+modal bertumpuk, visual focus/browser |
| SessionHistoryModal | Ada | Hook-order open/Escape; gap compact busy/failure, statistik fixture nyata, focus trap browser |
| SettingsModal | Ada | Reflection/compaction/skills race; gap validasi number min/max saat save non-form, provider fetch berpindah, unsaved/saving edits |
| Sidebar | Ada | Pin/search/snippet XSS/Git callback; gap result stale saat query baru gagal, menu rect sebenarnya, workspace switching saat modal |
| SideBySideDiff | Tidak ada | UI-15; parser hunks/metadata dan ukuran virtualizer/split pendek |
| SpotlightBar | Tidak ada | Prompt accept/reject ditelusuri; gap pending double-submit, keyboard/fokus, send options dan label Search vs prompt |
| SubagentTreeCard | Ada | Expand/artifact/cancelled; gap failed count/error details, timer aktif tanpa chunk, huge task text dan keyboard |
| TerminalOutput | Tidak ada | UI-16; tail berbasis geometri nyata, clipboard reject, ANSI/plain text, chunk besar |
| TimelineScrubber | Ada | Tests memakai scrollElement=null dan callback saja; gap browser scroll ratio, jumlah turn besar, navigasi keyboard/tooltip clipping |
| ToolsInspectorModal | Ada | Katalog native/MCP success; gap failure parsial/semua, server lambat, stale catalog setelah reload, fokus dan schema panjang |

**Infrastruktur test:** jsdom (`vitest.config.ts:8`) dan ResizeObserver stub tanpa measurement (`src/test/setup.ts:6-12`) tidak membuktikan layout/virtual scrolling. App dan semua test ChatCanvas memock MessageRow menjadi null, sehingga handler wiring lulus pun tidak memverifikasi rendering/error turn yang nyata. Sebanyak **10 komponen tidak memiliki file test langsung** (termasuk logo); daftar tabel di atas tidak berarti semuanya sama-sama berisiko.

## Regresi bernilai tertinggi untuk tahap berikutnya

1. **Boundary keselamatan dan mutasi:** UI-01/03/04/07. Provider terkontrol, dua workspace terpisah, message lama, modal aktif; buktikan tidak ada mutation tanpa scope/persetujuan yang benar.
2. **Dispatcher composer menyeluruh:** UI-02/05/06. Semua slash command melalui klik/Enter/Send/queue, opsi execution/Council/attachment utuh, request ditolak tidak membuang draft/item.
3. **Renderer/error receipt:** UI-09/15/18. Real MessageRow di App, startup failure, tool non-terminal gagal, split diff edge cases; teks error dan perubahan tidak boleh hilang.
4. **UI live minimum window:** UI-12/13/14/16. Native/browser measurements 900×600 dengan sidebar+shell+task cards+queue, fokus/keyboard, terminal chunks, scroll-up dan resize. Screenshot/rect assertions harus diberi provenance UI live, tidak diganti assertion className.
5. **Lifecycle native/portal:** async drag registration setelah unmount, artifact baru saat Live Shell dipilih, reflection inspector di row virtual, modal berlapis, clipboard permission error.

## Evaluasi kualitas laporan

| Sumbu | Skor | Bukti / batas perbaikan |
|---|---:|---|
| Akurasi | 4/5 | Wiring UI→App→hook→command dirujuk; belum ada reproduksi dinamis, layout diberi label source-derived |
| Kelengkapan | 4/5 | Seluruh 26 komponen, CSS, util, dan 18 test dibaca; interaksi WebView/OS masih perlu verifikasi live |
| Kejelasan | 4/5 | Temuan memiliki severity/lokasi/pemicu/dampak/fix/regresi; detail cukup panjang karena scope menyeluruh |
| Actionability | 4/5 | Fix minimum dan test spesifik tersedia; belum berupa patch karena tahap read-only |
| Keringkasan | 4/5 | Temuan dipisahkan dari observasi yang belum cukup pasti; tabel coverage menambah panjang tetapi diperlukan untuk memastikan scope |

Rata-rata **4,0/5**. Peningkatan utama: reproduksi boundary keselamatan dan data-loss terlebih dahulu, lalu ukur layout/keyboard di native minimum window. Penilaian ini tidak menyamakan audit source dengan verifikasi runtime. Workflow skill audit sudah tersedia; tidak membuat/mengubah skill tambahan karena izin tulis tugas dibatasi hanya laporan ini.
