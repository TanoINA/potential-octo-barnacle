# Bugfix Audit Round 2 — UI/UX + Backend (2026-11-06)

Dokumen ini mencatat semua temuan dan perbaikan dari ronde bugfixing menyeluruh
setelah fitur Self-Improvement Review selesai. Setiap fix backend dikerjakan
dengan siklus **strict TDD (tes RED dulu, lalu GREEN)** dan punya tes regresi.
Verifikasi dilakukan oleh parent (bukan hanya klaim subagent): gate penuh
dijalankan ulang, kode fix dibaca langsung, dan perilaku diuji di app nyata via
mock IPC (tanpa provider nyata / API berbayar).

## Ringkasan eksekutif

| Area | Temuan | Status |
|---|---|---|
| Backend Rust (search.rs, tools.rs) | 2 (P1 crash, P2 SSRF) | ✅ fixed + terverifikasi |
| Backend Rust (tools, skills, runner, search, storage, provider, mcp) | 14 (2 P2 + 12 P3) | ✅ fixed + terverifikasi |
| Frontend (Sidebar, speech, Settings, Artifacts) | 4 (P1 XSS, P2 ×3) | ✅ fixed + terverifikasi |
| Frontend wiring (App/ChatCanvas/useAgent) | 7 (P1, P2, P3 ×5) | ✅ fixed + terverifikasi |

**Total: 25 temuan diperbaiki, 24 tes regresi backend baru (73 → 97) + 17 tes frontend baru
(49 → 66), semua gate hijau dan diverifikasi ulang oleh parent. Tidak ada commit —
semua perubahan hanya di working tree.**

## A. Backend — round 1 (subagent `sa-0`, diverifikasi parent)

### A1. P1 — Panic char-boundary saat indexing sesi (crash app)
- **path:line:** `src-tauri/src/agent/search.rs:91`, `:109` (sebelum fix); helper `bounded_str` di `:58`, dipakai di `:104`, `:118`
- **Bug:** slicing buta `&res[..4000]` / `&art.content[..8000]` panic bila batas byte jatuh di tengah karakter UTF-8 multi-byte. Jalur reachable: `save_session` → `index_session`. Profile release memakai `panic = "abort"` → satu sesi dengan tool result multi-byte di posisi batas **meng-crash seluruh app**.
- **Fix:** truncation mundur ke batas karakter (`bounded_str`); seam `index_session_into` untuk tes hermetic.
- **Tes:** `indexing_multibyte_tool_output_and_artifacts_truncates_on_char_boundaries` (`search.rs:337`). RED: `panicked at src\agent\search.rs:95:48: end byte index 4000 is not a char boundary` → GREEN.

### A2. P2 — SSRF bypass lewat IPv4-mapped / 6to4 / Teredo
- **path:line:** `src-tauri/src/agent/tools.rs:1244` (`is_public_ip`; fix `:1256-1278`)
- **Bug:** `::ffff:127.0.0.1`, `::ffff:10.0.0.1`, `::ffff:169.254.169.254` dinilai publik → `fetch_web_page` → `validate_public_url` mengizinkan SSRF ke internal/link-local (cloud metadata). 6to4 (`2002::/16`) & Teredo (`2001::/32`) yang menyematkan IPv4 juga lolos.
- **Fix:** IPv4-mapped/6to4/Teredo dievaluasi ulang sebagai IPv4-nya; alamat publik palsu (`::ffff:8.8.8.8`) tetap diizinkan.
- **Tes:** `ipv4_mapped_and_embedded_ipv6_targets_cannot_bypass_the_private_filter` (`tools.rs:1626`) + `fetch_rejects_ipv4_mapped_loopback_literal_urls` (`tools.rs:1643`, chokepoint end-to-end). RED→GREEN.

## B. Backend — round 2 (subagent `sa-0-fe746d2d`, selesai + diverifikasi parent)

Baseline 73 tes → akhir **81 tes lulus / 0 gagal** (8 tes regresi baru).
`cargo test` di-run ulang oleh parent: **81 lulus / 0 gagal** — 3 tes PowerShell
yang sempat gagal di run subagent (child process timeout 20s di bawah beban
paralel) lolos saat di-run ulang; bukan regresi. `clippy -D warnings` dan
`fmt --check` bersih (verifikasi parent).

### B1. P2 — Lost update: penulisan file tanpa verifikasi snapshot
- **path:line:** `src-tauri/src/agent/tools.rs:501` (`write_file`), `:539` (`edit_file`); helper `guarded_atomic_write` di `:470`
- **Bug:** `atomic_write` dipanggil tanpa memverifikasi isi file yang sudah dibaca → hasil edit bisa menimpa perubahan di luar konteks (lost update).
- **Fix:** guard membaca ulang file tepat sebelum rename; snapshot tidak cocok → error `"...changed on disk...re-read it and retry"`, file tidak tersentuh; file yang muncul tiba-tiba juga ditolak.
- **Tes:** `guarded_write_rejects_stale_or_unexpected_disk_state` (`tools.rs:1688`). RED: snapshot kedaluwarsa diterima → GREEN.
- **Risiko (dari subagent, diterima):** guard kini menolak menimpa file existing non-UTF-8 atau >10 MiB — konsisten dengan `edit_file`; ada double-read untuk file ≤10 MiB.

### B2. P3 — `save_skill_markdown` tanpa validasi nama/ukuran
- **path:line:** `src-tauri/src/agent/skills.rs:230`
- **Bug:** nama skill tidak divalidasi (nama 65 karakter diterima), tanpa batas ukuran deskripsi/instruksi.
- **Fix:** memakai kontrak `reflection::validate_name` (kini `pub(crate)`); reserved device names Windows (`nul`, `con`, `com1`, ...), nama kosong/oversized, deskripsi >1024 B, instruksi kosong/>32 KiB → ditolak; nama normal tetap dinormalkan (`"Test Skill!"` → `test-skill`).
- **Tes:** `save_skill_markdown_rejects_unsafe_or_oversized_names` (`skills.rs:381`).

### B3. P3 — Path developer hardcoded `D:\hermes-agent-main\*` di default skill roots
- **path:line:** `src-tauri/src/agent/skills.rs:13` (`default_skill_paths`)
- **Bug:** default skill roots meng-embed path mesin dev → pengguna lain memuat direktori yang tidak ada / bocor struktur mesin dev.
- **Fix:** hanya root berbasis home (`~/.aether-agent/skills`, `~/.gemini/config/skills`).
- **Tes:** `default_skill_paths_do_not_include_machine_specific_directories` (`skills.rs:381`).

### B4. P3 — Celah SSRF: rentang reserved/CGNAT/benchmark tidak diblokir
- **path:line:** `src-tauri/src/agent/tools.rs:1317` (`is_public_ip`; blok `:1328-1347`)
- **Bug:** `240.0.0.0/4`, `100.64.0.0/10` (CGNAT), `198.18.0.0/15`, `192.0.0.0/24`, `192.88.99.0/24`, `2001:db8::/32` (dokumentasi), `64:ff9b::/96` (NAT64) dinilai publik.
- **Fix:** semua rentang di atas diblokir; NAT64 dialirkan IPv4 embedded-nya ke filter yang sama; IP publik normal tetap diizinkan.
- **Tes:** `reserved_benchmark_and_shared_ipv4_ranges_are_blocked` + `ipv6_documentation_and_nat64_targets_cannot_bypass_the_private_filter`.

### B5. P3 — `write_file` salah menghitung statistik "Files Changed"
- **path:line:** `src-tauri/src/agent/tools.rs:449` → helper `diff_line_stats` di `:453`
- **Bug:** `additions = lines().count().max(1)`, `deletions: 0` → menimpa file selalu menampilkan deletions 0 dan additions terlalu besar.
- **Fix:** diff nyata (baris identik = Equal), konvensi sama dengan `edit_file`.
- **Tes:** `write_file_reports_accurate_diff_stats_for_overwrite`.

### B6. P3 — Deep search: satu sesi ramai menggeser sesi lain dari hasil
- **path:line:** `src-tauri/src/agent/search.rs:195` → seam `search_query_with` di `:154`
- **Bug:** `LIMIT 100` baris di level SQL → sesi dengan 150 baris match memakan seluruh limit, sesi lain hilang dari hasil.
- **Fix:** agregasi atas hingga 10.000 baris lalu dipotong `limit` per sesi; `match_count` benar, urutan skor tetap ascending; perilaku penguncian DB tidak berubah.
- **Tes:** `deep_search_does_not_let_one_session_crowd_out_other_sessions`.

### B7. P3 — `redact_provider_error` hanya meredaksi `api_key`
- **path:line:** `src-tauri/src/agent/runner.rs:1278` (4 call site diperbarui agar mengoper `&ProviderConfig`)
- **Bug:** kredensial di `extra_headers` (mis. `Bearer tok-123`) bocor ke event error yang tampil di UI.
- **Fix:** memakai `reflection::provider_secrets` — `api_key`, header auth/key/token/secret, dan kredensial scheme `Bearer`/`Basic` jadi `[redacted]`; header non-sensitif (`X-Trace-Id`) dipertahankan untuk debugging.
- **Tes:** `provider_errors_redact_credentials_from_extra_headers` (`runner.rs:1347`).

### B8. P2 — Pin session "hidup" lagi setelah dilepas (fix parent)
- **path:line:** `src-tauri/src/agent/storage.rs:404` (sebelum fix); helper `merge_pin_state` di `:397-409`, varian `save_session_unlocked_with_pin_policy` di `:411-420`
- **Bug:** `save_session_unlocked` merestore pin dari disk bila `data.session.pinned_at.is_none() && previous.session.pinned`. `toggle_pin_session` (:464) melepas pin dengan menulis `pinned=false, pinned_at=None` lewat jalur save yang sama → kondisi restore terpenuhi terhadap file disk yang belum tertimpa → **pin yang baru dilepas langsung aktif kembali**. Setiap unpin jadi no-op senyap.
- **Fix:** keputusan merge diekstraksi ke `merge_pin_state(data, previous, pin_is_authoritative)`; save biasa tetap melindungi pin dari payload default (`false`), sementara `toggle_pin_session` memakai `pin_is_authoritative = true` sehingga state pin eksplisit menang.
- **Tes:** `unpinning_a_session_is_not_resurrected_by_stale_disk_state` (`storage.rs:983`). RED: E0425 (helper belum ada) → GREEN: payload hasil toggle tidak di-repin, save biasa tetap mempertahankan pin.
- **Catatan:** subagent round 2 kehilangan daftar temuan asli lagi (truncation konteks) dan mengaudit sendiri; temuan ini dikerjakan parent setelah verifikasi langsung ke kode.

### B9. P3 — `use_skill` diabaikan untuk tool call subagent
- **path:line:** `src-tauri/src/agent/runner.rs` — seam `dispatch_subagent_tool` (:147) + helper bersama `activate_skill` (:117); blok `use_skill` loop utama tetap (:702)
- **Bug:** dispatch tool untuk subagent tidak mengenali `use_skill` → model yang memanggil skill di dalam konteks subagent dapat "Unknown tool: use_skill"; subagent juga tidak memuat katalog skill ke system prompt-nya.
- **Fix:** dispatch subagent dan loop utama berbagi `activate_skill`; subagent memuat katalog skill dan menyuntikkannya ke prompt.
- **Tes:** `subagent_use_skill_tool_call_activates_the_catalogued_skill`. RED (runtime): `Unknown tool: use_skill` → GREEN.
- **Risiko:** subagent membaca SKILL.md dari katalog workspace + path global lewat jalur tervalidasi; SKILL.md besar tetap dibatasi 256 KiB.

### B10. P3 — `SseDecoder::failed` tidak dipropagasi ke caller
- **path:line:** `src-tauri/src/agent/provider.rs` — `SseDecoder`/`fail` (:906-926), caller stream (:361-380)
- **Bug:** caller tidak pernah membaca `decoder.failed`; jaminan error bergantung penuh pada efek samping `fail()` (memancarkan event error). Bila mekanisme itu berubah, stream gagal bisa terbaca sukses (senyap).
- **Fix:** konstanta `SSE_EVENT_TOO_LARGE` (:429); caller memeriksa `decoder.failed` setelah `feed` (break) dan setelah `finish` (return Err); feed/consume_line berhenti saat gagal.
- **Bukti:** mutation-tested — `fail()` dimutasi agar tidak memancarkan event error, tes `openai_http_stream_surfaces_oversized_sse_event_as_error` tetap lulus karena cek eksplisit; tanpa fix tes itu gagal.

### B11. P3 — SSE streaming ditandai selesai tanpa cek `finish_reason`
- **path:line:** `src-tauri/src/agent/provider.rs` — `finish_reason` di `StreamResult` (:37)/`StreamAccumulator` (:71), `validate_stream_result` (:118), `capture_finish_reason` (:631)
- **Bug:** jalur streaming menandai selesai pada `[DONE]` tanpa membaca `finish_reason` dan tanpa memastikan ada konten → run terpotong limit token atau kena `content_filter` tampil "sukses" tanpa indikasi.
- **Fix:** `finish_reason` ditangkap dari OpenAI (`choices[0].finish_reason`) dan Anthropic (`message_delta.delta.stop_reason` / `stop_reason` non-stream); validator mengembalikan error eksplisit bila reason tidak dikenal atau tidak ada konten/reasoning/tool call; diterapkan di jalur streaming dan non-streaming.
- **Tes:** 5 tes baru (`reports_truncated_finish_reason_as_error`, `reports_content_filter_finish_reason_as_error`, `rejects_empty_completion`, `accepts_stop_finish_reason`, `accepts_tool_calls_finish_reason`) + 2 tes unit. RED via mutasi validator → GREEN.
- **Risiko:** perubahan perilaku paling terasa — run terpotong kini gagal dengan error, bukan sukses; provider tanpa `finish_reason` (beberapa Ollama) tidak terpengaruh; `stop`/`tool_calls` normal tetap sukses (dijaga tes anti-false-positive).

### B12. P3 — MCP tool alias melebihi batas nama fungsi provider
- **path:line:** `src-tauri/src/agent/mcp.rs` — `build_alias` (:330) + `MAX_ALIAS_CHARS = 64` (:326)
- **Bug:** alias `mcp_<id>_<name>` bisa >64 karakter (mis. 103 chars untuk server/tool bernama panjang) → OpenAI & Anthropic menolak function name >64 char → **seluruh request gagal**. Alias terbit vs alias diterima dispatcher, deskripsi/schema, dan penanganan error juga diverifikasi konsisten dengan native tools.
- **Fix:** alias di-cap deterministik di 64 karakter; suffix dedup tetap bekerja dan dijamin unik (dibuktikan untuk suffix 2..=2000, anti infinite-loop).
- **Tes:** `mcp_aliases_stay_within_the_provider_function_name_limit`, `mcp_alias_dedup_survives_the_length_cap`, `capped_mcp_aliases_stay_unique_for_every_dedup_suffix`, `short_mcp_aliases_are_not_reshaped`, `mcp_aliases_schema_and_errors_match_the_native_tool_contract`. RED (runtime): alias 103 char diterima → GREEN 5/5.
- **Risiko:** alias yang sudah tersimpan di riwayat percakapan lama berubah bentuk untuk server MCP bernama sangat panjang — model perlu memanggil ulang dengan alias baru.

## C. Frontend — subagent `sa-1` (diverifikasi parent)

### C1. P1 — Stored XSS di snippet deep-search Sidebar
- **path:line:** `src/components/Sidebar.tsx:180` (blok 177–186 sebelum fix); helper `renderSnippetWithHighlights` di `:26-59`, dipakai di `:220`
- **Bug:** `dangerouslySetInnerHTML` pada snippet FTS berisi pesan/artifact tersimpan (tak tepercaya); hanya `<mark>` yang di-replace, sisanya di-parse sebagai HTML → payload tersimpan dieksekusi saat korban search.
- **Fix:** semua teks di-escape; hanya `<mark>`/`</mark>` jadi elemen React.
- **Tes:** `src/components/Sidebar.test.tsx:128-204`. RED: `<img src="x" onerror="...">` jadi elemen DOM nyata → GREEN 4/4. `grep dangerouslySetInnerHTML src/` = **0**.

### C2. P2 — Teks speech-to-text terduplikasi
- **path:line:** `src/hooks/useSpeechRecognition.ts:35-49`
- **Bug:** `onresult` meneruskan hasil interim (berubah tiap chunk audio) + menggabung final+interim dalam satu event.
- **Fix:** hanya `result.isFinal` yang diteruskan.
- **Tes:** `src/hooks/useSpeechRecognition.test.tsx` (baru, 3 tes). RED: interim `'hello'` terkirim + `'fix thisand this'` → GREEN 3/3.

### C3. P2 — Race `list_skills` di SettingsModal
- **path:line:** `src/components/SettingsModal.tsx:86-93` (sebelum); fix `:170-186`
- **Bug:** effect deteksi skill tanpa cancellation; tiap ketikan di skill roots memicu invoke, respons lama menimpa hasil baru.
- **Fix:** flag `active` + cleanup effect.
- **Tes:** `src/components/SettingsModal.test.tsx:139-167`. RED: skill dari respons stale muncul → GREEN 7/7.

### C4. P2 — Kegagalan muat Artifacts Library senyap
- **path:line:** `src/components/ArtifactsLibraryModal.tsx:87-89` (sebelum); loader+cancellation `:65-103`, UI error + Retry `:249-262`
- **Bug:** `catch` kosong; load gagal menampilkan "No artifacts have been generated yet" (empty-state palsu, menyesatkan).
- **Fix:** state error + pesan alasan + tombol Retry.
- **Tes:** `src/components/ArtifactsLibraryModal.test.tsx:114-135`. RED: tidak ada UI error → GREEN 4/4.

## D. Frontend wiring — slice parent (App.tsx / ChatCanvas.tsx / useAgent.ts)

Tes baru: `src/components/ChatCanvas.commands.test.tsx` (11 tes) + tambahan di
`src/App.test.tsx` + `src/hooks/useAgent.reflection.test.tsx`. RED 11 tes → GREEN 24/24.

### D1. P1 — Council Mode mati (settings tidak diteruskan)
- **path:line:** `src/App.tsx` (wiring `settings={agent.settings}` ke ChatCanvas)
- **Bug:** `ChatCanvas` mengirim semua pesan lewat jalur single-model; `councilMode` hanya kosmetik. `effectiveConfig` dibaca dari `props.settings` yang tak pernah dikirim App.
- **Fix:** App meneruskan `settings` (composition root); tes assert `sendMessage` dipanggil dengan `{council: true, candidateA, candidateB}`.
- **QA app nyata:** toggle council enabled dengan 2 model, banner "Dual Model Comparison", combobox A/B bernilai beda, `send_council_message` ter-invoke.

### D2. P2 — 5 komponen ber-tes tapi tak pernah di-mount
- **path:line:** `src/App.tsx` (state + render modal/panel)
- **Bug:** `SessionHistoryModal`, `ToolsInspectorModal`, `MemoryModal`, `SpotlightBar`, `InteractiveTerminal` punya tes hijau tapi tak pernah muncul di app (opener tidak di-wire).
- **Fix:** state `historyOpen/toolsOpen/memoryOpen/spotlightOpen/liveShellOpen` + opener ChatCanvas (`onOpenHistory`, `onOpenTools`, `onOpenMemory`, `onOpenSpotlight`, `onOpenLiveShell`) + render modal di App; `InteractiveTerminal` dapat tombol close; `aria-label="dialog"` pada modal.
- **Tes:** opener `/history`, `/tools`, `/memory`, spotlight, live shell semuanya RED→GREEN.

### D3. P2 — Composer no-op (thinking, model, subagent, skills)
- **path:line:** `src/App.tsx` (props ChatCanvas)
- **Bug:** toggle thinking mode, quick model switch, indikator subagent/skill, dan tombol skills tidak melakukan apa pun (prop tidak di-wire).
- **Fix:** `onUpdateOverride` → `updateRunConfig`, `onSelectModel` → `selectModel`, `subagents`, `activeSkills`, `onListSkills` diteruskan.

### D4. P3 — `/stop` dibisukan guard streaming
- **path:line:** `src/components/ChatCanvas.tsx` (`send()` — flag `interruptsRun`)
- **Bug:** semua slash command (termasuk `/stop`) diabaikan saat `isStreaming` → run yang hang tak bisa diinterupsi dari composer.
- **Fix:** `/stop` dikecualikan dari guard; slash matching via `allSlashItems.find`.
- **QA app nyata:** `/stop` saat streaming → `abort_agent` ter-invoke, composer cleared.

### D5. P3 — Council B fallback ke model aktif
- **path:line:** `src/components/ChatCanvas.tsx` (`councilSecondCandidate`)
- **Bug:** council B default bisa sama dengan model aktif → "dual model" jadi single model tanpa disadari.
- **Fix:** kandidat B difilter agar ≠ model aktif maupun council A; tombol Council `disabled` + title dinamis "Council Mode needs at least two configured models" bila <2 model.

### D6. P3 — Caret setelah insert mention `@`
- **path:line:** `src/components/ChatCanvas.tsx` (`handleAddMention`)
- **Bug:** `setCursor(prev + 2)` salah posisi setelah insert mention.
- **Fix:** `setCursor(next.length)`.

### D7. P3 — Notifikasi preview menampilkan prompt user
- **path:line:** `src/hooks/useAgent.ts` (~baris 387)
- **Bug:** notifikasi "Task Complete" memakai pesan terakhir apa pun, termasuk prompt user sendiri (membocorkan isi prompt di preview OS).
- **Fix:** hanya dari `m.role === 'assistant'`.

## Gate (diverifikasi parent)

| Gate | Hasil |
|---|---|
| `npm test` | **66/66 (13 file)** |
| `npx tsc --noEmit` | bersih |
| `npm run build` | lulus (index chunk 454.53 kB → gzip 126.18 kB) |
| `cargo test --manifest-path src-tauri/Cargo.toml` | **97 tes, semua lulus** (baseline 73 + 24 regresi baru; re-run parent) |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | lulus (re-run parent) |
| `cargo fmt --check` | lulus (re-run parent) |

Catatan: 3 tes PowerShell (`tools.rs`) sesekali gagal dengan `Elapsed(())` saat
suite penuh dijalankan di bawah beban paralel berat (child process PowerShell
kena timeout 20s) — lulus konsisten saat di-run isolasi (3/3, 5.5s) dan sudah
lulus di beberapa full-run. Bukan regresi: tes-tes itu tidak disentuh oleh fix
apa pun di ronde ini.

## Catatan jujur / gap yang diketahui

1. **5–8 temuan P3 frontend dari audit asli hilang saat kompaksi konteks** — daftar lengkap hanya tersimpan di transcript audit yang entri-nya terpotong. Yang teridentifikasi sudah ditangani (9 di atas + 3 temuan tambahan parent); sisanya tidak bisa dipulihkan dari sumber yang ada.
2. **Browser QA**: verifikasi app nyata mencakup boot, council (banner + A/B beda + `send_council_message`), dan `/stop` → `abort_agent` saat streaming. Daemon browser sempat mati berulang kali di bawah beban (subagent paralel), sehingga QA panel `/history` `/tools` `/memory` spotlight live-shell di app nyata tertunda — ketiganya sudah dicover deterministik oleh tes komponen (App.test.tsx + ChatCanvas.commands.test.tsx, 24/24 GREEN).
3. **Tidak ada commit** — repo punya banyak perubahan lokal user yang dipertahankan; semua fix hanya di working tree.
4. `search.rs` untracked di git sehingga tidak muncul di `git diff` — fix diverifikasi langsung ke kode.

## Cara verifikasi ulang

```bash
# frontend
npm test
npx tsc --noEmit && npm run build
# backend
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --check --manifest-path src-tauri/Cargo.toml
# app nyata (tanpa API berbayar): mock IPC di
# C:/Users/Tano/AppData/Local/hermes/cache/scratch/aether-reflection/qa-mock.js
npm run dev -- --host 127.0.0.1 --port 1421 --strictPort
```
