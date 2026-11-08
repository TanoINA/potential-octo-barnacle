# P04 — Provider-native assistant state

## Hasil dalam scope worker

- `StreamResult.assistant_content: Vec<serde_json::Value>` menyimpan blok Anthropic dalam urutan index native. Accumulator menggunakan `BTreeMap<usize, Value>`.
- `process_anthropic_event` menyalin setiap `content_block_start` utuh (termasuk `redacted_thinking` dan field tambahan), kemudian menggabungkan `text_delta`, `thinking_delta`, dan `signature_delta` secara verbatim ke blok terkait. Signature tidak diturunkan dari teks reasoning dan tidak ditrim/di-decode.
- `process_anthropic_message` juga mempertahankan blok nonstream asli. `finish()` hanya mengisi `tool_use.input` dari fragmen JSON tool yang valid; blok thinking/redacted tidak direkonstruksi.
- `convert_anthropic_messages` mempertahankan native content array dan tidak menggandakan `tool_use` ketika `tool_calls` legacy dengan ID sama ikut hadir.
- Helper publik: `StreamResult::assistant_message(&self, kind: ProviderKind) -> serde_json::Value`.
  - Anthropic dengan native blocks: `{role:"assistant", content:[...native blocks...]}`. Tidak menambah `tool_calls` duplikat.
  - Provider lainnya/fallback: `{role:"assistant", content:<display text>, tool_calls?:[...]}`.
  - DeepSeek saja: tambahkan `reasoning_content` bila reasoning tidak kosong. Tidak membocorkan field ini ke OpenAI/Ollama/Anthropic.
  - `stream_index` tetap serde-skip pada tool calls.

## Kontrak integrasi runner/frontend (belum dikerjakan worker ini)

Rust `ChatMessage` mempunyai field opsional baru:

```rust
#[serde(rename = "providerMessages", default, skip_serializing_if = "Option::is_none")]
pub provider_messages: Option<Vec<serde_json::Value>>,
```

TypeScript: `providerMessages?: Record<string, unknown>[]`.

Event baru:

```rust
AgentEvent::ProviderState(ProviderStateEventData {
    run_id: String,
    session_id: String,
    provider_messages: Vec<serde_json::Value>,
})
```

Wire:

```json
{"type":"provider_state","data":{"runId":"run-1","sessionId":"session-1","providerMessages":[]}}
```

Runner perlu menggunakan helper untuk **setiap** assistant response, termasuk assistant final tanpa tools; kumpulkan assistant/tool transcript milik display message/run, lalu emit snapshot `provider_state`. Frontend menyimpan snapshot tersebut pada ChatMessage aktif sesuai run/session, termasuk saat save/reload. Pada reload, runner harus menggunakan transcript ini sebagai pengganti rekonstruksi dari teks UI/tool cards. Worker ini tidak mengubah runner/hooks/storage.

State tersimpan bukan sumber instruksi tepercaya: validasi role hanya `assistant`/`tool`, kaitkan tool result dengan tool call yang benar, dan **jangan** mengizinkan replay system/user dari `providerMessages`. Jangan menyertakan system prompt atau user history di snapshot event. Pertimbangkan pembatasan provider asal ketika user mengganti adapter; field opaque native Anthropic bukan format lintas-provider yang aman dikirim mentah.

Semua literal Rust `ChatMessage { ... }` lama perlu `provider_messages: None`; error kompilasi terakhir menunjuk `search.rs` dua literal dan `storage.rs` dua literal. Parent/owner file harus memperbaruinya. Periksa juga exhaustive match `AgentEvent` saat integrasi.

## Verifikasi nyata

- Test pertama ditulis sebelum implementasi. `cargo test --manifest-path src-tauri/Cargo.toml --lib p04_anthropic_signed_stream_survives_tool_roundtrip -- --nocapture` menghasilkan RED kompilasi yang diharapkan: `assistant_content`/`assistant_message` belum tersedia, **bersama** error intermediate worker lain. Ini bukan RED perilaku yang terisolasi.
- Percobaan Cargo sesudah implementasi masih terblokir error intermediate `skills.rs`, `runner.rs`, serta literal ChatMessage belum dimigrasikan; tidak ada klaim full-crate GREEN. Setelah instruksi pause, tidak ada lagi Cargo maupun penulisan `provider.rs`.
- Alternatif terisolasi: `C:/Users/Tano/AppData/Local/hermes/cache/scratch/p04_harness.py` menyalin region production terkini secara persis ke harness Rust di scratch, ditambah `types.rs` asli dan file test baru. Tidak mengganti logic dengan mock dan tidak menambah Cargo dependency. Harness memakai pasangan rlib serde/serde_json kompatibel yang sudah ada di target.
- Perintah `python C:/Users/Tano/AppData/Local/hermes/cache/scratch/p04_harness.py`: **4 passed, 0 failed, 7 filtered out**. Meliputi signed streamed thinking + signature terfragmentasi + redacted block + tool result roundtrip; DeepSeek helper dan isolasi field provider; nonstream opaque extension dan deduplikasi tool; serialization ChatMessage legacy/native serta scope event.
- `rustfmt --edition 2021 src-tauri/src/agent/provider_state_audit_tests.rs` sukses. Tidak memformat provider.rs bersama karena ownership paralel.

## File yang disentuh worker

- `src-tauri/src/agent/provider.rs`: hanya region struct/helper/finish, converter Anthropic, processor Anthropic, serta deklarasi module test di dekat StreamResult. Parent harus mempertahankan perubahan SSE worker lain jika menggabungkan file ini.
- `src-tauri/src/agent/types.rs`: optional persistence field dan event schema.
- `src/types/agent.ts`: optional field dan event union.
- Baru: `src-tauri/src/agent/provider_state_audit_tests.rs`.
- Baru: laporan ini.

Tidak ada commit, dependency baru, perubahan runner/hooks, atau live request provider. P04 baru selesai di boundary provider/schema; acceptance request kedua + reload end-to-end bergantung integrasi parent.

## Self-check

Akurasi 4/5: empat pure test GREEN dari source production, belum full crate. Kelengkapan 3/5: runner/frontend/reload end-to-end belum diintegrasikan sesuai ownership. Kejelasan 4/5: kontrak helper/event eksplisit, masih perlu provider-origin policy parent. Actionability 4/5: helper siap, literal lama harus dimigrasikan. Keringkasan 4/5: detail integrasi sengaja disimpan di laporan, bukan final ringkas. Perbaikan prioritas: integrasikan transcript tervalidasi, migrasikan literal, lalu rerun focused Cargo dan request-roundtrip/reload tests setelah tree stabil.
