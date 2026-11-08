# Audit layer provider dan tools

## Batas dan status verifikasi

- Audit statis, **tanpa mengubah source, tanpa build, tanpa menjalankan test atau skenario mutatif**. Parent memiliki gate build/test. Skenario regresi di bawah adalah usulan, bukan hasil eksekusi.
- Scope utama dibaca: `src-tauri/src/agent/{provider,tools,search,mcp,pty,git,mod}.rs`. Call path ditelusuri secara terbatas melalui `runner.rs`, `types.rs`, `storage.rs`, `tasks.rs`, `lib.rs`, `InteractiveTerminal.tsx`, dan `FilesChangedCard.tsx`.
- Root terverifikasi `D:/AetherAgent`. Tidak ditemukan `AGENTS.md` pada root/ancestor scope maupun daftar tracked Git. Source scope utama bersih pada pemeriksaan scoped `git status`; tidak ada perubahan source oleh audit ini.
- Satu-satunya file yang dibuat adalah laporan ini. File evidence milik parent tidak dibaca atau diubah. Tidak ada kredensial/config pengguna yang dibaca.
- Beberapa `read_file` dan pencarian rekursif mengalami timeout; pembacaan dilanjutkan dengan Python read-only melalui terminal. Tidak ada hasil runtime aplikasi yang diasumsikan dari fallback tersebut.
- Severity: **H** = kehilangan data/batas keamanan/kegagalan fitur utama; **M** = salah hasil, resource leak, atau fitur gagal pada input valid tertentu. Tidak ada temuan C yang dapat dipastikan secara statis.

## Temuan prioritas tinggi

### P01 — H — Path kanonis Windows direduksi menjadi basename, sehingga Stage/Revert bisa mengenai file lain

**Lokasi:** `src-tauri/src/agent/tools.rs:561-564`, `639-642`; asal canonical path `1521-1545`; konsumen `git.rs:189-218`, `247-255`.

**Trigger/call path:** Workspace tersimpan sebagai `D:\repo` (storage menghapus prefix extended pada `storage.rs:968-971`), lalu `write_file`/`edit_file` pada `src/config.ts`. Resolver menghasilkan `\\?\D:\repo\src\config.ts`, tetapi `strip_prefix(cwd)` membandingkan bentuk extended dengan bentuk biasa dan gagal; fallback mengirim **`config.ts`**, bukan `src/config.ts`. `FilesChangedCard.tsx:60-82` meneruskan `filesChanged.path` itu ke Stage/Revert dengan cwd root.

**Dampak:** Semua file dengan basename sama digabung dalam kartu (`FilesChangedCard.tsx:24-37`). Stage gagal atau men-stage file root yang salah. Revert dapat menimpa/menghapus `D:\repo\config.ts` padahal yang diedit `D:\repo\src\config.ts`.

**Perbaikan minimal:** Hitung relative path terhadap **root canonical yang sama** dengan resolver; kembalikan pasangan `{root, resolved, relative}` dari resolver atau canonicalize cwd sebelum `strip_prefix`. Jangan fallback ke basename jika konversi gagal.

**Test regresi:** Windows workspace non-extended; buat file root dan nested dengan basename sama; edit nested; pastikan `filesChanged.path == src/config.ts`, Stage/Revert hanya menyentuh nested. Sertakan cwd case-varian dan UNC.

### P02 — H — Snapshot guard tidak menserialisasi write paralel: dua edit sukses, satu perubahan hilang

**Lokasi:** `src-tauri/src/agent/tools.rs:508-536`; call sites `545-547`, `643`; pemanggilan paralel `runner.rs:1067-1077`, `1283`.

**Trigger/call path:** Model mengembalikan dua `edit_file` untuk file sama dalam satu turn. Keduanya membaca snapshot A, keduanya melewati `read_existing_text` di guard sebelum salah satu rename, lalu masing-masing memanggil `storage::atomic_write` (temp/write/sync/rename di `storage.rs:910-934`).

**Dampak:** Kedua tool dilaporkan sukses tetapi rename terakhir menggantikan hasil edit pertama. Re-read sebelum rename bukan compare-and-swap dan tidak menutup race dengan editor eksternal.

**Perbaikan minimal:** Per-path async mutex yang mencakup **read awal → replace/diff → guarded write** untuk writer aplikasi; serialisasi tool mutatif pada file sama. Untuk jaminan terhadap editor eksternal diperlukan strategi file locking/revision/conditional replace yang benar, bukan klaim CAS dari dua pembacaan.

**Test regresi:** Barrier dua writer setelah pembacaan guard; hasil harus kedua edit terserialisasi atau satu konflik eksplisit, bukan dua success dengan perubahan hilang. Uji writer eksternal di antara recheck dan rename.

### P03 — H — Konfigurasi Anthropic thinking mengirim temperature yang tidak valid dan effort tanpa capability gate

**Lokasi:** `src-tauri/src/agent/provider.rs:589-603`; nilai default `types.rs:170-174`; model default `storage.rs:55-61`.

**Trigger/call path:** Pilih Anthropic, aktifkan thinking, gunakan temperature default 0.7. `runner.rs:831-836` meneruskan nilai ke `anthropic_body`, yang tetap mengirim `temperature: 0.7` bersama manual extended thinking. API manual thinking tidak mendukung modifikasi temperature seperti ini. Secara terpisah, semua model menerima `output_config.effort` walaupun dukungan bergantung model; enum internal juga mengirim `minimal`, `extra_high`, `ultra` mentah.

**Dampak:** Request ditolak HTTP 400 sebelum stream berjalan pada kombinasi model/parameter tersebut. Sebagian model lama tanpa effort gagal walaupun thinking dimatikan. Model adaptive-only juga tidak bisa dipakai hanya dengan toggle ini karena selalu dipaksa `type: enabled/disabled`.

**Perbaikan minimal:** Adapter capability per model/provider: omit temperature saat extended thinking; hanya kirim effort yang didukung dan map ExtraHigh ke wire value yang benar; manual budget hanya untuk model pendukung, adaptive untuk model adaptive-only. Jika `maxTokens < 2048`, jangan diam-diam menaikkan cap user (`591-592`): validasi konfigurasi atau jelaskan constraint.

**Test regresi:** Snapshot payload model Sonnet 4.5/manual dan model adaptive-only, thinking on/off, temperature 0.7, semua effort, cap token kecil; negative fixture menolak field unsupported. Live smoke menggunakan akun parent, bukan diasumsikan sudah lolos.

### P04 — H — Thinking state/signature hilang saat round-trip tool Anthropic dan DeepSeek

**Lokasi:** `src-tauri/src/agent/provider.rs:68-74`, `852-860`, `913-936`, `649-665`; penerusan `runner.rs:983-987`, `1767-1785`.

**Trigger/call path:** Provider mengeluarkan thinking → tool call → tool result → request lanjutan. Accumulator hanya menyimpan string reasoning. Anthropic `signature_delta` dan `redacted_thinking` diabaikan; `StreamResult` tidak membawa blok asli. Runner memasukkan assistant berisi content/tool_calls saja; converter Anthropic membangun ulang text/tool_use tanpa thinking block. DeepSeek `reasoning_content` berhasil dibaca (`provider.rs:767-775`) tetapi tidak disisipkan kembali dalam assistant messages.

**Dampak:** Anthropic kehilangan blok thinking lengkap yang wajib direplay ketika melanjutkan tool use; DeepSeek thinking + tools mewajibkan replay `reasoning_content`, sehingga request lanjutan dapat ditolak HTTP 400. Riwayat reload juga menghapus reasoning saat dibuat menjadi provider messages.

**Perbaikan minimal:** Simpan structured provider-native assistant state (blok thinking + signature/redacted block dan `reasoning_content`) terpisah dari teks tampilan; teruskan utuh ke request berikut dan persistence. Jangan merekonstruksi signature dari string reasoning.

**Test regresi:** Mock dua round untuk Anthropic (thinking_delta + signature_delta + tool_use) dan DeepSeek (reasoning_content + tool_calls); tangkap request kedua dan reload-history request, assert state wajib tetap identik.

### P05 — H — Adapter OpenAI mengirim field lintas-provider tanpa pembatasan

**Lokasi:** `src-tauri/src/agent/provider.rs:563-578`; enum wire `types.rs:27-37`.

**Trigger/call path:** Konfigurasi `kind: openai`, endpoint resmi OpenAI. Payload selalu berisi `thinking` (ekstensi DeepSeek), `reasoning_effort`, `temperature`, dan menggunakan `max_tokens` untuk semua model. Ini juga terjadi ketika thinking off. Nilai ExtraHigh menjadi `extra_high`, bukan `xhigh`.

**Dampak:** Endpoint/model strict menolak field atau effort unsupported; model o-series tidak kompatibel dengan `max_tokens` dan butuh `max_completion_tokens`. Model non-reasoning tidak boleh dipaksa menerima reasoning control; beberapa model reasoning melarang temperature. Label OpenAI-compatible tidak menjamin semua ekstensi diterima.

**Perbaikan minimal:** Pisahkan payload resmi OpenAI, DeepSeek, Ollama, dan custom-compatible dengan capability allowlist. Pilih token-limit key dan temperature berdasarkan model; mapping enum ke wire values per adapter. `extraBody` tetap escape hatch sadar pengguna, bukan default yang mencampur protokol.

**Test regresi:** Strict request-validator fixtures untuk GPT-4o/non-reasoning dan o-series, plus semua effort; pastikan OpenAI tidak menerima `thinking`, o-series memakai key token yang benar, dan ExtraHigh tidak dikirim sebagai `extra_high`.

### P06 — H — EOF SSE parsial dianggap sukses dan dapat mengeksekusi tool dari turn yang belum selesai

**Lokasi:** `src-tauri/src/agent/provider.rs:120-140`, `531-553`; fallback SSE `458-470`.

**Trigger/call path:** Server menutup HTTP secara normal sesudah satu delta teks atau satu tool delta lengkap secara JSON, tetapi sebelum `finish_reason`/`[DONE]`/`message_stop`. EOF hanya `break`; validator membolehkan `finish_reason: None` selama ada content/reasoning/tool. Contoh satu event `{"choices":[{"delta":{"content":"jawaban sebagian"}}]}` lalu EOF diterima. Varian tool memakai arguments valid tetapi belum terminal.

**Dampak:** Runner menandai final answer berhasil (`runner.rs:976-980`) atau menjalankan tool (`983` dan seterusnya), walaupun stream terpotong. Cabang invalid UTF-8 (`provider.rs:1175-1176`) juga diam-diam membuang baris; EOF di tengah multibyte bisa kehilangan delta terakhir tanpa error bila ada teks sebelumnya.

**Perbaikan minimal:** Track terminal state eksplisit per protokol; EOF tanpa terminal harus error retryable/incomplete, bukan success. `[DONE]`/finish_reason/`message_stop` harus ditangani secara konsisten. Invalid UTF-8 di data event harus menjadi decoding error. Mode kompatibilitas provider yang benar-benar omit terminal harus opt-in, bukan bypass global.

**Test regresi:** Teks parsial + EOF; tool args valid + EOF tanpa terminal (assert tool tidak dijalankan); truncation di setiap byte UTF-8; clean terminator tetap sukses. Existing test `1637-1642` justru mengunci perilaku permisif ini.

### P07 — H — Cap SSE tidak membatasi total stream dan dapat dilewati dengan data line kosong

**Lokasi:** `src-tauri/src/agent/provider.rs:1138-1148`, `1180-1197`, `773`, `783`, `1023`; cap nonstream `224`, `371`.

**Trigger/call path:** Provider mengirim `data:\n` berulang tanpa blank event delimiter. `data.len() == 0`, maka `data_bytes` tidak bertambah tetapi setiap baris mendorong `String` baru ke `data_lines`. Skenario lain mengirim banyak event kecil valid: `content`, `reasoning`, tool arguments dan jumlah tool terus bertambah. Byte heartbeat/progress cukup untuk mereset stale timeout.

**Dampak:** Memori tumbuh tanpa batas walaupun setiap event terlihat di bawah 1 MiB. Batas response 10 MiB hanya dipakai nonstream. Aplikasi dapat kehabisan memori dari endpoint salah/bermasalah.

**Perbaikan minimal:** Batasi bytes wire/line count per event (termasuk delimiter dan overhead), batas jumlah tool, serta total decoded response/stream duration. Cek batas sebelum extend/alokasi, bukan sesudah seluruh chunk dimasukkan.

**Test regresi:** Ribuan/ratusan ribu data line kosong dalam satu event; stream delta kecil yang melewati total cap; jumlah tool berlebihan; semua harus menghasilkan error bounded tanpa membekukan UI.

### P08 — H — MCP read tanpa cap, sedangkan timeout tidak mencakup write stdin

**Lokasi:** `src-tauri/src/agent/mcp.rs:54-58`, `109-121`, `134-162`; publikasi result `302-314` dan `runner.rs:1141-1165`.

**Trigger/call path:** Server mengeluarkan stdout tanpa newline atau satu JSON result sangat besar. `BufReader.lines().next_line()` mengalokasikan line tanpa batas sebelum parsing. Server yang berhenti membaca stdin membuat `write_all`/lock stdin menggantung; timeout 20 detik baru dipasang setelah write selesai.

**Dampak:** OOM dari MCP lokal/remote wrapper yang salah, atau tool/connect UI menggantung tak terbatas. Timeout receiver bukan deadline end-to-end. Result besar yang valid juga diteruskan utuh ke artifact/conversation, tanpa bounded output native tools.

**Perbaikan minimal:** Bounded frame reader (byte chunks + maksimum line/result), timeout/cancellation untuk seluruh request termasuk menunggu stdin mutex, write, flush, dan response; cap tool-result sebelum publikasi. Simpan reader JoinHandle dan hentikan transport saat framing gagal.

**Test regresi:** MCP server flood tanpa newline; JSON result di atas cap; server tidak membaca stdin dengan arguments melampaui pipe capacity; assert selesai error dalam deadline dan proses/reader dibersihkan.

### P09 — H — Daemon command melewati byte cap runner foreground

**Lokasi:** `src-tauri/src/agent/tools.rs:977-987` → `src-tauri/src/agent/tasks.rs:172-193`, `220-228`.

**Trigger/call path:** `run_command` dengan `isDaemon: true` menjalankan proses yang menulis satu baris tak berakhir, atau baris sangat panjang berulang. Cabang daemon langsung keluar sebelum reader chunked dan cap 1 MiB foreground (`tools.rs:1023`, `1202-1234`). TaskManager memakai `lines()` dan hanya membatasi **jumlah baris** menjadi 500.

**Dampak:** Satu baris tanpa newline memakan memori tak terbatas sebelum dikirim; 500 baris raksasa tetap tak bounded; log disk juga bertambah tanpa batas. Saran memakai daemon untuk watcher/server justru membuka jalur output yang lebih berisiko.

**Perbaikan minimal:** Gunakan reader chunked bounded bersama foreground, ring buffer berdasarkan bytes, rotasi/kuota log disk, serta batas status-result. Byte cap harus diterapkan pada cabang daemon, bukan hanya foreground.

**Test regresi:** Daemon stdout tanpa newline melebihi cap dan stream stderr raksasa; ukur retained bytes, status payload, serta disk cap. Tidak dijalankan pada audit read-only ini.

### P10 — H — Revert/Stage memakai pathspec Git, bukan path file literal

**Lokasi:** `src-tauri/src/agent/git.rs:189-218`, `225-227`, `247-255`.

**Trigger/call path:** Pada Windows file legal `docs/[ab].txt` dipilih untuk Revert; repo juga mempunyai `docs/a.txt` modified. Argumen setelah `--` masih ditafsirkan Git sebagai pathspec glob, bukan nama literal. Input magic seperti `:(top)**` juga tidak ditolak. Tidak ada resolver confinement; `../sibling.txt` dapat mengacu ke bagian lain dari repo induk jika workspace adalah subfolder.

**Dampak:** Revert satu file dapat menimpa perubahan file lain; Stage satu file dapat memasukkan perubahan lain. Batas workspace file tools tidak berlaku pada command Git ini. Ini bukan shell injection: `--` hanya menghentikan option parsing, bukan pathspec magic.

**Perbaikan minimal:** Validasi path terhadap workspace dan repo root tanpa traversal (untuk deleted paths validasi ancestor); gunakan `git --literal-pathspecs ... -- <relative>` atau `:(literal)` yang dibentuk aman; larang kosong/root/directory jika operasi memang per-file. Jangan `trim_start_matches` untuk mengubah absolute path menjadi path lain.

**Test regresi:** Dua tracked file `docs/[ab].txt` dan `docs/a.txt` dengan perubahan berbeda; revert/stage pertama tidak menyentuh kedua. Tolak magic pathspec, `../`, drive luar, UNC luar; semua skenario di disposable repo.

## Temuan medium

### P11 — M — Ollama thinking off tidak diterapkan ke endpoint yang dipakai

**Lokasi:** `src-tauri/src/agent/provider.rs:323-328`, `566-577`.

**Trigger/call path:** Ollama thinking-capable model, `thinking: false`, effort default medium. Request dikirim ke `/v1/chat/completions` dengan `think: false`, `thinking: {type:disabled}`, tetapi juga `reasoning_effort: medium`.

**Dampak:** Handler OpenAI-compatible Ollama memetakan `reasoning_effort`/`reasoning.effort`, bukan native `think`; unknown fields diabaikan. Akibatnya thinking tetap diminta medium walaupun toggle off. ExtraHigh juga dikirim `extra_high`, yang tidak ada dalam daftar accepted compatibility aliases (`xhigh` tersedia), sehingga bisa 400.

**Perbaikan minimal:** Pada OpenAI-compatible Ollama kirim `reasoning_effort: none` untuk off, supported effort untuk on; hapus native-only control. Atau gunakan native chat dengan decoder/payload yang sesuai. Discovery `/api/show` membantu mapping model-defined levels.

**Test regresi:** Tangkap request on/off; off harus `none`; ExtraHigh wire `xhigh`/mapped supported value; verifikasi model yang mendukung disable tidak mengembalikan reasoning.

### P12 — M — Anthropic message_stop tidak mengakhiri stream; framing CR-only juga tidak dipahami

**Lokasi:** `src-tauri/src/agent/provider.rs:478-520`, `939-947`, `1143-1148`.

**Trigger/call path:** Server mengirim Anthropic `message_stop` lalu mempertahankan koneksi atau heartbeat. Callback hanya menandai done untuk `[DONE]`/error, sedangkan `process_anthropic_event` mengabaikan message_stop. Varian SSE sah memakai delimiter CR tunggal, bukan LF/CRLF: decoder hanya mencari LF.

**Dampak:** Jawaban lengkap menunggu EOF; koneksi idle memberi timeout 60 detik dan menggagalkan turn sukses, heartbeat dapat menahannya lebih lama. CR-only menumpuk seluruh stream sebagai satu baris dan gagal JSON/size limit.

**Perbaikan minimal:** Jadikan terminal event Anthropic sinyal done; decoder line ending mendukung CR, LF, CRLF termasuk split antarchunk. Callback harus berhenti mengolah event tambahan setelah terminal agar hasil tidak bergantung coalescing chunk.

**Test regresi:** message_stop dengan socket tetap terbuka; heartbeat sesudah terminal; semua split CRLF, CR-only; `[DONE]` diikuti bytes lain dalam chunk sama tidak boleh mengubah result.

### P13 — M — MCP discovery hanya membaca halaman pertama

**Lokasi:** `src-tauri/src/agent/mcp.rs:167-174`; pemakai `224-264`.

**Trigger/call path:** `tools/list` mengembalikan `{tools:[...], nextCursor:"page2"}`. `list_tools` mengembalikan array pertama saja dan membuang cursor.

**Dampak:** Sebagian tool tidak pernah dipublikasikan ke provider dan tidak dapat didispatch, walaupun konfigurasi server terlihat sukses.

**Perbaikan minimal:** Ikuti `nextCursor` dengan params `cursor`, akumulasi dengan batas jumlah halaman/tools dan deteksi cursor berulang. Gunakan satu implementasi untuk manager dan UI list.

**Test regresi:** Mock tools/list dua halaman; schema/deskripsi/alias semua halaman tersedia dan callable; cursor cycle/oversize menjadi error eksplisit.

### P14 — M — Timeout/drop request MCP tidak membatalkan pekerjaan server, dan partial connect tidak punya cleanup terstruktur

**Lokasi:** `src-tauri/src/agent/mcp.rs:140-162`, `188-205`, `217-275`; caller cancellation `runner.rs:758-764`, `1129-1131`.

**Trigger/call path:** Tool server bekerja lebih dari 20 detik; timeout hanya menghapus pending entry. Atau user membatalkan saat beberapa server sudah connected dan manager masih menunggu server berikut. `tokio::select!` menjatuhkan connect/call future. Tidak ada cancellation notification/drop guard request; manager tidak memiliki cleanup guard untuk clients yang sudah dimulai. `kill_on_drop` hanya parent process, bukan jaminan process tree.

**Dampak:** Tool dapat melanjutkan side effect setelah error timeout dan dipanggil ulang oleh model; partial connect/drop bisa meninggalkan descendant proses. Pada request future drop, pending sender juga tinggal sampai respons/stdio EOF karena cleanup normal tidak dieksekusi.

**Perbaikan minimal:** RAII pending-request guard; kirim `notifications/cancelled` sesuai protocol dan jangan menyebut timeout sebagai rollback. Cleanup lease untuk manager/transport yang menghentikan process tree meski connect future dibatalkan; reader task dikelola eksplisit. Di Windows pertimbangkan Job Object untuk ownership descendants.

**Test regresi:** Cancel di antara dua server connect dan ketika tools/call berjalan; assert pending kosong, cancellation diterima, dan parent/child/grandchild tidak tersisa. Timeout side-effect harus terdokumentasi sebagai indeterminate, tidak aman diretry otomatis.

### P15 — M — Foreground command yang parent-nya keluar lebih dahulu meninggalkan descendant

**Lokasi:** `src-tauri/src/agent/tools.rs:1050-1057`, `1089-1108`, `1185-1199`; shared caller `pty.rs:25-35`.

**Trigger/call path:** PowerShell meluncurkan child/grandchild dan shell keluar lebih awal. Loop mendahulukan `try_wait`, lalu hanya drain 500 ms dan abort reader. `terminate_child` hanya dipanggil pada cancel/timeout ketika shell masih hidup. Pada Unix helper itu juga hanya kill child, tanpa process group.

**Dampak:** Command non-daemon dapat dilaporkan selesai tetapi proses turunan tetap berjalan, menahan file/port atau melakukan write sesudah run berhenti. Setelah PID shell hilang, `taskkill /T` berbasis parent PID bukan mekanisme ownership yang andal.

**Perbaikan minimal:** Windows Job Object dengan kill-on-close yang menaungi seluruh proses; Unix process group/session. Tentukan lifetime children secara eksplisit: non-daemon tidak boleh terlepas; daemon harus melalui manager. Deadline tree termination sendiri juga harus bounded.

**Test regresi:** Parent keluar cepat, child menahan stdout/port dan grandchild sleep; pastikan non-daemon tidak menyisakan semuanya. Ulangi timeout/cancel serta no-newline flood; existing foreground tests hanya memeriksa direct PowerShell.

### P16 — M — Error git status menjadi laporan Working tree clean

**Lokasi:** `src-tauri/src/agent/git.rs:72-106`.

**Trigger/call path:** `rev-parse` sukses, tetapi `git status` exit nonzero (misalnya index corrupt/akses ditolak). `.output()` sukses spawn sehingga kode meneruskan stdout kosong tanpa mengecek `status_out.status.success()`; branch exit status juga diabaikan.

**Dampak:** UI menampilkan repo bersih/HEAD alih-alih error. Pengguna membuat keputusan commit/push berdasarkan status palsu.

**Perbaikan minimal:** Periksa exit status setiap subprocess; propagate stderr terbatas. Bedakan git executable gagal, bukan repo, dan repo bermasalah.

**Test regresi:** Stub git menghasilkan rev-parse sukses lalu status exit 128 + stderr; command harus `Err`, tidak `hasChanges:false`/clean. Branch nonzero juga harus jelas.

### P17 — M — Parser porcelain merusak rename, quoted UTF-8, dan path relatif subfolder

**Lokasi:** `src-tauri/src/agent/git.rs:22-34`, `85-92`.

**Trigger/call path:** Status rename `R  old.txt -> new.txt` menghasilkan satu `path` literal `old.txt -> new.txt`. Nama Unicode dengan default `core.quotePath` dikirim Git sebagai C-escaped octal; parser hanya menghapus quote, tidak decode. `git status --porcelain` selalu menghasilkan path relatif repo root, tetapi fungsi menerima cwd subfolder tanpa mengembalikan informasi root.

**Dampak:** Field structured `files.path` bukan nama file sebenarnya; konsumen yang menggunakannya untuk open/stage/revert gagal atau memilih scope salah. Current FilesChangedCard memakai stats tools, jadi temuan parser ini bukan sumber utama P01.

**Perbaikan minimal:** Gunakan `--porcelain=v1 -z`, parse NUL records dan record tambahan rename secara benar; tampilkan old/new path terpisah. Normalisasi terhadap repo root yang diketahui, lalu konversi eksplisit ke workspace-relative.

**Test regresi:** Rename, nama ber-spasi, non-ASCII, kurung siku, serta cwd subfolder; byte path output harus round-trip tanpa reparsing display string.

### P18 — M — Commit/push Git tidak bounded dan tidak dapat dibatalkan

**Lokasi:** `src-tauri/src/agent/git.rs:123-143`, `162-166`; command factory `mod.rs:17-21`.

**Trigger/call path:** Git hook commit/push meminta input atau menggantung; SSH credential flow tidak selesai; hook mengeluarkan stdout/stderr tanpa batas. Semua cabang memakai `.output().await` tanpa deadline, cap, cancellation token, atau kill-on-drop/tree policy. `create_hidden_command` hanya menyembunyikan window.

**Dampak:** UI Commit/Push tetap loading, invisible prompt tidak dapat dijawab; output dapat menghabiskan memori. Menutup/menjatuhkan pemanggilan tidak menjamin process tree berhenti.

**Perbaikan minimal:** Shared bounded subprocess runner dengan deadline, cancellation, process ownership, batas output, dan error stderr yang jelas. Atur kebijakan prompt Git/SSH non-interaktif dan surface auth requirement tanpa membuka prompt tersembunyi.

**Test regresi:** Disposable repo hook infinite sleep, hook flood tanpa newline, mock push auth hang; operasi harus bounded, dapat dicancel, tidak menyisakan descendants.

### P19 — M — Deep search masih membatasi raw rows sebelum agregasi session

**Lokasi:** `src-tauri/src/agent/search.rs:190-198`, `225-249`.

**Trigger/call path:** Session A mempunyai lebih dari 10.000 row matching ber-rank tinggi, session B mempunyai satu matching row lebih rendah; caller meminta beberapa session. SQL memotong raw results sebelum dedupe session, lalu Rust baru agregasi/truncate.

**Dampak:** Session B hilang walaupun jumlah session hasil di bawah limit, dan `matchCount` A undercount. Existing regresi `404-447` hanya menggunakan 150 row sehingga tidak menyentuh limit 10.000.

**Perbaikan minimal:** Agregasikan jumlah/best rank per session sebelum limit hasil, dengan query bertahap/CTE yang menjaga aturan auxiliary FTS5; ambil best snippet untuk session terpilih. Jika memakai paging, lanjutkan sampai cukup distinct session dan hitung count secara terpisah. Jangan menamai hit count exact kalau dibatasi scan cap.

**Test regresi:** A berisi 10.001+ row matching singkat dan B satu row panjang; semua session yang diminta tetap ditemukan, matchCount benar. Sertakan limit kecil/zero dan SQL row error; `rows.flatten()` saat ini juga menyembunyikan error iterasi.

### P20 — M — Validasi public URL rentan DNS rebinding karena koneksi melakukan resolve ulang

**Lokasi:** `src-tauri/src/agent/tools.rs:1319-1322`, `1426-1437`.

**Trigger/call path:** `fetch_web_page` memvalidasi hostname lewat `lookup_host`, memperoleh IP publik, lalu `reqwest.get(url)` melakukan lookup sendiri. DNS yang dikendalikan target dapat mengembalikan private/loopback pada lookup kedua; redirect divalidasi tetapi memiliki race sama.

**Dampak:** Jaminan tool “private targets rejected” tidak berlaku terhadap DNS rebinding: request dapat menjangkau service lokal/private dari mesin pengguna. Risiko dibatasi bahwa tool shell memang berprivilege luas; ini tetap bug batas keamanan fetch, bukan klaim sandbox aplikasi keseluruhan.

**Perbaikan minimal:** Pin koneksi ke IP hasil validasi via resolver/connector khusus sambil mempertahankan host/SNI; validasi semua candidate dan final peer, gunakan kebijakan proxy yang konsisten. Setiap redirect membangun target tervalidasi baru.

**Test regresi:** Resolver mock menjawab public saat validate lalu private saat connect; pastikan tidak ada request private. Uji DNS multi-address dan redirect juga.

## Coverage dan batas klaim

- **SSE:** Jalur JSON/nonstream, fallback SSE mislabeled, stream chunking, CRLF, EOF flush, split UTF-8, tool accumulation, terminal/finish reason, error event, stale timeout, serta size cap dibaca. Split UTF-8 normal sebelum newline disimpan sebagai bytes dan existing test mencakupnya; tidak dilaporkan sebagai bug lama yang sudah diperbaiki. Yang tersisa: P06/P07/P12.
- **Payload:** `CompletionOptions` run utama/council memang meneruskan setting ke provider. Problem terdapat pada encoding/capability dan state replay (P03–P05/P11), bukan tuduhan semua opsi dibuang. `extraBody` mengoverride field explicit; dianggap escape hatch sadar pengguna, tidak dinilai bug sendiri.
- **File/path Windows:** Resolver menolak no-workspace, traversal relatif, dan memeriksa canonical ancestor untuk target belum ada; symlink escape existing yang sederhana telah ditangani. Namespace `\\?\`/`\\.\` ditolak pada input, tetapi resolver mengeluarkan canonical namespace paths. P01 adalah kegagalan end-to-end yang jelas. `validate_relative_path` hanya dijalankan untuk input relatif sehingga absolute path melewati filter reserved/trailing-dot/ADS; konsekuensi spesifik Win32 perlu fixture Windows dan **tidak dinaikkan menjadi exploit terkonfirmasi**. Prefix `Path::starts_with` bukan perbandingan case-insensitive Win32; UNC/case/8.3/reparse-TOCTOU tetap wajib diuji, bukan diasumsikan lulus. Ancestor canonical check bukan handle-bound confinement bila reparse point ditukar setelah validasi.
- **Eksekusi/PTY:** Foreground sudah memakai chunked reader, bounded channel, display cap, timeout/cancel, serta taskkill `/T` pada cancel Windows. Tidak dilaporkan seolah masih memakai unbounded line reader. P09 adalah cabang daemon berbeda; P15/P18 process ownership terpisah. `pty.rs` ternyata facade one-shot noninteractive, bukan PTY riil: stdin null, output gabungan, token cancel baru tidak terdaftar untuk UI. Tidak ada bukti bahwa persistent `cd`/interactive stdin dijanjikan oleh kontrak saat ini; tidak dihitung temuan tersendiri.
- **MCP:** Alias sanitization/dedupe/64-char cap serta `isError` handling runner sudah ada; tidak dilaporkan sebagai bug lama. Frame limits, deadline write, pagination, cancellation/drop cleanup masih bermasalah.
- **Git:** Argumen commit tidak melewati shell; temuan P10 ialah pathspec semantics, bukan shell injection. Commit `add -A` memang documented stage-all; tidak dianggap bug hanya karena dapat memasukkan semua perubahan.
- **Search:** UTF-8 safe truncation indexer sudah ada. Batas raw-row sebelum grouping masih menyisakan P19. Tidak ada klaim panic multibyte lama.

## Urutan gate yang disarankan untuk parent

1. Windows disposable workspace: P01 canonical-path identity dan P10 literal pathspec (risiko salah file/data loss), lalu P02 concurrent writer.
2. Request-capture dua round Anthropic/DeepSeek dan strict per-provider validators (P03–P05/P11).
3. HTTP SSE fixtures: EOF parsial, split UTF-8, CR-only, message_stop tanpa EOF, byte/event cap (P06/P07/P12).
4. Mock MCP + process tree fixtures: giant frame, blocked stdin, multi-page tools, cancel during connect/call (P08/P13/P14).
5. Foreground/daemon grandchild + no-newline output + hanging Git hooks (P09/P15/P18).
6. Git error/porcelain fixtures, FTS multi-session di atas raw-row cap, resolver-pinned web fetch (P16/P17/P19/P20).

Semua gate tersebut **belum dijalankan oleh subagent** sesuai instruksi read-only/no-build/no-test.

## Referensi protokol yang diperiksa

- OpenAI Chat API: https://platform.openai.com/docs/api-reference/chat/create ; referensi token-limit/effort menyatakan dukungan model berbeda dan `max_tokens` tidak kompatibel o-series: https://platform.openai.com/docs/api-reference/chat/create?lang=python
- Anthropic extended thinking (temperature dan preserving complete thinking blocks/signatures): https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking
- Anthropic effort/capability: https://docs.anthropic.com/en/docs/build-with-claude/effort
- DeepSeek thinking + tools / mandatory `reasoning_content` replay: https://api-docs.deepseek.com/guides/thinking_mode
- Ollama OpenAI compatibility: https://docs.ollama.com/api/openai-compatibility ; implementation field/mapping checked: https://raw.githubusercontent.com/ollama/ollama/main/openai/openai.go (ChatCompletionRequest, thinkFromReasoningEffort, FromChatRequest).
- MCP tools pagination: https://modelcontextprotocol.io/specification/2024-11-05/server/tools
- Git porcelain/root-relative and rename format: https://git-scm.com/docs/git-status ; pathspec semantics: https://git-scm.com/docs/gitglossary

## Evaluasi singkat deliverable

| Sumbu | Skor | Bukti/batas |
|---|---:|---|
| Akurasi | 4/5 | Lokasi dan call path diperiksa langsung; belum ada reproduksi runtime, compatibility bergantung model/server aktual. |
| Kelengkapan | 4/5 | Seluruh tujuh modul scope dan boundary terkait dibaca; UNC/case/reparse race belum dieksekusi. |
| Kejelasan | 4/5 | Temuan memakai trigger–impact–fix–test; bagian coverage diperlukan untuk membedakan bug baru dari perbaikan existing. |
| Actionability | 4/5 | Setiap temuan punya minimal fix dan gate konkret; parent tetap perlu menulis/menjalankan fixture. |
| Keringkasan | 4/5 | Style-only ditolak; laporan panjang karena merinci boundary yang berbeda, bukan menggabungkan semua failure sebagai satu bug. |

Prioritas peningkatan: jalankan tiga gate pertama, turunkan/konfirmasi severity berdasarkan reproduksi, dan uji UNC/case pada filesystem Windows nyata sebelum klaim parity. Tidak membuat skill baru karena izin write dibatasi hanya laporan ini.
