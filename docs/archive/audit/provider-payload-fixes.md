# Perbaikan payload provider — P03/P05/P11

## FINAL residual slice — supersedes historical handoff below

**P03 capability adapter selesai. Exclusive provider.rs write lock RELEASED.** Module test sudah diregistrasi; parent boleh mengedit provider.rs kembali.

### Verified outcomes

- Aplikasi asli RED awal: `cargo test --manifest-path src-tauri/Cargo.toml provider_payload -- --nocapture` → **6 passed; 2 failed**. Failures benar: effort Opus 4.5 hilang dan off Opus 4.7 masih memakai temperature.
- Setelah adaptive adapter: seluruh **8 tests awal GREEN**. Switch-provider regression baru terlihat RED karena native signed blocks dikirim mentah. Responses-only regression terlihat RED karena canceled request lolos validasi model; helper background test sempat compile RED sebelum helper ditambah.
- Payload final: command sama → **11 passed; 0 failed; 174 filtered out**.
- Final setelah stale existing wire assertion diperbarui: `cargo test --manifest-path src-tauri/Cargo.toml agent::provider:: -- --test-threads=1` → **41 passed; 0 failed; 0 ignored; 144 filtered out; finished in 0.45s**. Ini mencakup 11 payload, 4 replay/state, 5 SSE audit, dan seluruh existing provider tests. Tidak ada warning dari cargo run final.
- `rustfmt --edition 2021 src-tauri/src/agent/provider_payload_audit_tests.rs` exit 0. Tidak menjalankan full native gate, commit, menambah deps, atau request authenticated upstream.

### Implementation

- Capability table hanya explicit documented aliases dan Anthropic `-YYYYMMDD`; unknown/custom/future IDs tetap legacy. Manual Sonnet/Haiku 4.5 tanpa effort; manual Opus 4.5 low/medium/high. Opus/Sonnet 4.6 adaptive low/medium/high/max (ExtraHigh→high). Model modern dalam matrix di bawah adaptive low/medium/high/xhigh/max. **Mythos Preview mendukung max tetapi tidak xhigh** (ExtraHigh→high).
- Adaptive tanpa budget mempertahankan cap positif 512; nol error; minimum 2048 hanya manual. Temperature omitted saat thinking-on dan seluruh mode model modern yang menolak sampling.
- Off 4.6/4.7/4.8/Sonnet 5 explicit disabled; Opus 5/Haiku 5.5 hanya high ke bawah. Always-thinking models dan Sonnet 5.5 off error lokal jujur, tidak silently enable/between_tools.
- **Parent reflection integration:** tersedia `pub fn supports_thinking_off(provider: &ProviderConfig, effort: ReasoningEffort) -> bool`. Untuk background Low gunakan `thinking: !provider::supports_thinking_off(provider, ReasoningEffort::Low)`. Tidak mengubah chat/UI options diam-diam. Helper non-Anthropic/unknown mempertahankan legacy adapter contract, bukan jaminan reasoning benar-benar off di semua model.
- `validate_chat_model(...) -> Result<(), String>` di stream boundary menolak official GPT-5 Pro/GPT-5.2 Pro alias+dated snapshots sebelum client/network dengan error Responses API. Custom hosts dan unknown future IDs tidak diblokir.
- Target OpenAI-compatible adapter menormalisasi native Anthropic thinking/redacted/tool_use/tool_result ke text + OpenAI tool_calls + tool-role results; preserve existing tool messages; reasoning_content hanya untuk DeepSeek. Source messages/signed blocks tidak dimutasi, native Anthropic converter tidak diubah. Roundtrip switch regression GREEN.
- Existing `request_contains_thinking_and_effort`: assertion `extra_high`→`xhigh` dengan komentar wire mapping. `merge_extra_body` tetap terakhir. Parent SSE/state helper edits tidak diubah.

### Docs re-verified and remaining scope

Directly fetched official `https://platform.claude.com/docs/en/build-with-claude/thinking.md` dan `https://platform.claude.com/docs/en/build-with-claude/effort.md`: adaptive/off/sampling table dan max-vs-xhigh availability diverifikasi. Official HTML `https://developers.openai.com/api/docs/models/gpt-5-pro` dan `https://developers.openai.com/api/docs/models/gpt-5.2-pro` menyatakan Responses API only (Markdown endpoints hanya stub). Raw scratch evidence: `C:/Users/Tano/AppData/Local/hermes/cache/scratch/provider-capability-docs/`.

Parent full suite proc_c157 sudah exit sebelum build baru. Satu build kemudian menunggu cargo directory lock dan sukses. Patch auto-linter memberi false-positive Rust-2015 async errors; cargo edition-2021 compile/test final sukses. Parent menangani reflection fixture dan helper integration. Effective API-version `2026-10-21` override handling belum dibuat; default header tetap `2023-06-01`. Unknown model discovery, official GPT-5.3+/GPT-6 support, o3-pro detector, dan native multimodal switch conversion bukan scope ini. No full release-readiness claim.

Files residual slice: provider.rs, provider_payload_audit_tests.rs, laporan ini. Tidak ada edit reflection/runner/types/SSE/state helper. Self-evaluation 4/5 masing-masing accuracy/completeness/clarity/actionability/conciseness (overall 4.0): 41 provider tests + official docs, namun live API dan final native gates belum dilakukan; parent reflection integration masih diperlukan. Highest-impact follow-up: integrate helper + final native gate. Tidak menyimpan global skill karena task file scope.

---

## Historical handoff (retained as evidence, superseded above)

## Status penting untuk parent

**P05 dan P11 sudah diimplementasi; P03 baru parsial.** Semua write `provider.rs` dihentikan segera setelah pesan lock parent. Tidak ada cargo/build sesudah pesan tersebut. Jangan menganggap seluruh acceptance sudah GREEN.

Snapshot source terakhir yang dibaca:

- `openai_body` membedakan endpoint resmi hanya jika parsed host tepat `api.openai.com` dan kind Openai; bukan substring host. Custom-compatible termasuk GutsAI tetap membawa `thinking`, `reasoning_effort`, `temperature`, `max_tokens`; hanya `ExtraHigh` diubah menjadi `xhigh`.
- Model resmi non-reasoning tidak menerima `thinking`/`reasoning_effort`. o-series dan alias GPT-5/5.1/5.2 yang dikenal memakai `max_completion_tokens`; temperature dihapus saat reasoning; GPT-5.1/5.2 off memakai `none`, GPT-5 awal memakai `minimal`, o-series memakai `low` karena model tersebut tidak mendukung off penuh. Tidak ada klaim toggle off mematikan reasoning pada semua model.
- Ollama `/v1/chat/completions`: off `reasoning_effort: none`, on kompatibilitas effort termasuk `xhigh`; field native `think` dan vendor `thinking` tidak dikirim.
- Anthropic manual: temperature dihapus saat thinking, output_config effort mentah dihapus, cap user tidak dinaikkan diam-diam. Nol ditolak; cap <2048 dengan manual thinking ditolak. Off model legacy masih memakai temperature dan cap kecil yang positif.
- `merge_extra_body` tetap terakhir dan tidak diubah.

## Verifikasi nyata sebelum lock

Untuk menghindari cargo pada workspace parent yang sedang diedit, saya membuat harness Rust terisolasi di:

`C:/Users/Tano/AppData/Local/hermes/cache/scratch/provider-payload-harness`

`sync.py` membaca source asli dan mengekstrak persis `CompletionOptions`, ProviderConfig/ProviderKind/ReasoningEffort, fungsi payload + helper/converter pesan, serta text helper ke crate kecil. Ini **test fungsi produksi yang disalin verbatim**, bukan mock, namun **bukan build/full-suite aplikasi**.

Perintah nyata yang dijalankan:

```text
python C:/Users/Tano/AppData/Local/hermes/cache/scratch/provider-payload-harness/sync.py
cargo test --manifest-path C:/Users/Tano/AppData/Local/hermes/cache/scratch/provider-payload-harness/Cargo.toml --offline provider_payload -- --nocapture
```

Urutan RED→GREEN yang terlihat:

1. `provider_payload_official_non_reasoning_has_no_vendor_controls`: RED pada vendor `thinking`/`extra_high` di GPT-4o; GREEN setelah isolasi host resmi.
2. `provider_payload_official_reasoning_uses_supported_efforts_and_completion_cap`: RED pada o3 (effort hilang dan masih max_tokens); GREEN setelah mapping effort/key/temperature.
3. `provider_payload_ollama_compatibility_controls_and_custom_wire_mapping`: RED `minimal` saat off, seharusnya none; GREEN setelah adaptasi Ollama dan xhigh.
4. `provider_payload_anthropic_manual_thinking_validates_without_raising_cap`: RED karena temperature masih ada; GREEN setelah manual validation/omit unsupported fields.

**Hasil terakhir sebelum lock: `4 passed; 0 failed`, tanpa warning pada run terakhir.** Sebelumnya harness memberi dead_code warnings ketika fixture Anthropic belum dipakai; bukan warning aplikasi.

Setelah lock, file test ditambah acceptance Anthropic adaptive/off dan regresi escape hatch/DeepSeek; tidak dijalankan atas instruksi parent. `rustfmt --edition 2021 --check src-tauri/src/agent/provider_payload_audit_tests.rs` exit 0. File kini memiliki 8 test functions; hanya empat pertama telah dieksekusi RED→GREEN. Case GPT-5.2 Pro telah dikeluarkan dari acceptance test setelah docs resmi menyatakan Responses-only.

## P03 residual yang harus diintegrasi parent

Di `anthropic_body` saat ini masih semua thinking-on memakai manual `enabled` dan semua effort ditiadakan. Dua test acceptance baru **belum dieksekusi** dan diperkirakan masih gagal sampai parent menambahkan capability adapter:

- `provider_payload_anthropic_capabilities_map_all_efforts`
- `provider_payload_anthropic_off_is_explicit_or_rejected_not_faked`

Implementasikan allowlist model yang benar-benar dikenal, termasuk snapshot Anthropic `-YYYYMMDD`; jangan prefix longgar yang menerima `claude-sonnet-4-50-custom`.

| Model | Thinking-on | Effort yang didukung / mapping internal |
|---|---|---|
| Sonnet/Haiku 4.5 dan legacy | manual enabled+budget | omit output_config.effort |
| Opus 4.5 | manual enabled+budget | low/medium/high; Minimal→low; ExtraHigh/Max/Ultra→high |
| Opus/Sonnet 4.6 | adaptive tanpa budget | low/medium/high/max; Minimal→low; ExtraHigh→high; Max/Ultra→max |
| Opus 4.7/4.8 dan model 5+ yang didokumentasi | adaptive tanpa budget | low/medium/high/xhigh/max; Minimal→low; ExtraHigh→xhigh; Ultra→max |

- Validation cap <2048 hanya untuk manual, bukan adaptive. Adaptive wajib mempertahankan cap positif termasuk 512.
- Omit temperature saat thinking-on **dan** setiap mode pada Opus 4.7/4.8, Opus/Sonnet 5/5.5, Haiku 5.5, Fable/Mythos yang melarang sampling parameters.
- Off 4.6/4.7/4.8 dan Sonnet 5: `disabled`.
- Off Opus 5 dan Haiku 5.5: disabled hanya effort high ke bawah; xhigh/max harus error eksplisit atau perlu keputusan UX yang jujur, jangan diam-diam mengaktifkan reasoning.
- Off Opus 5.5, Sonnet 5.5, Fable 5/5.1, Mythos 5/5.1/Preview: **error eksplisit berisi model dan thinking** karena off penuh tidak didukung. Sonnet 5.5 punya between_tools pada high ke bawah tetapi bukan off penuh; jangan menyamakan keduanya tanpa perubahan kontrak UX.
- Models API capability discovery tidak dibuat dalam scope ini.

## P05 residual/batas jangkauan

- Allowlist OpenAI kini alias/snapshot tepat untuk o1/o1-mini/o1-preview/o3/o3-mini/o4-mini, gpt-5/-mini/-nano/-pro, 5.1, 5.2/-pro. Perlu menambah o3-pro ke detection list bila hendak mendukungnya; helper effort memuat cabang o3-pro tetapi detector belum memasukkannya.
- **GPT-5 Pro dan GPT-5.2 Pro resmi adalah Responses-only.** Payload helper dapat membangun bentuk field tetapi jalur aplikasi masih Chat Completions. Jangan klaim dukungan end-to-end model tersebut. Pilihan scope berikutnya: validasi reject model Responses-only pada boundary yang bisa mengembalikan Result atau implementasi Responses terpisah. Signature `openai_body` tidak diubah di scope ini.
- GPT-5.3/5.4/5.5/5.6 dan GPT-6 belum dideteksi sebagai reasoning oleh allowlist ini; jangan mengklaim general support seluruh keluarga. Model tak dikenal tidak diberi reasoning_effort default, tapi field token/temperature masih legacy sehingga bisa ditolak upstream. Lebih aman menambah capability eksplisit dengan docs daripada mengasumsikan semua prefix gpt-5 sama.
- Thinking-off untuk o-series/GPT-5 awal adalah effort terendah, bukan jaminan non-reasoning.
- Custom GutsAI sengaja tidak dipaksa strict model allowlist OpenAI. DeepSeek behavior lama tetap, mapping xhigh diuji sebagai regresi; native DeepSeek replay bukan scope ini.
- Extra_body masih override terakhir untuk request yang lolos local validation. Override max_tokens pada extra_body tidak membatalkan validasi options manual yang sudah error; keputusan override validation tetap perlu pembahasan bila diinginkan.

## Integrasi module test

Source test baru belum dideklarasikan di `provider.rs` karena parent memegang lock. Tambahkan di area module tests:

```rust
#[cfg(test)]
#[path = "provider_payload_audit_tests.rs"]
mod provider_payload_audit_tests;
```

Kemudian saat workspace koheren, jalankan gate parent:

```text
cargo test --manifest-path src-tauri/Cargo.toml provider_payload -- --nocapture
```

Dua test tambahan `provider_payload_extra_body_is_last_explicit_override` dan `provider_payload_deepseek_preserves_existing_thinking_controls` belum dieksekusi; tidak boleh dilaporkan pass sampai parent menjalankannya.

## Evidence docs resmi yang di-fetch sendiri

- OpenAI Chat create: https://platform.openai.com/docs/api-reference/chat/create — reasoning_effort bergantung model; max_tokens tidak kompatibel o-series; max_completion_tokens mencakup reasoning+visible output.
- GPT-5 guide: https://platform.openai.com/docs/guides/gpt-5.md — minimal/low/medium/high untuk GPT-5 awal.
- GPT-5.1 model: https://platform.openai.com/docs/models/gpt-5.1 — none/low/medium/high.
- GPT-5.2 model: https://developers.openai.com/api/docs/models/gpt-5.2.md — none/low/medium/high/xhigh, Chat supported.
- GPT-5.2 parameter compatibility: https://developers.openai.com/api/docs/guides/latest-model/gpt-5.2.md — temperature hanya none untuk 5.1/5.2; GPT-5 awal tidak menerima parameter sampling tersebut.
- GPT-5.2 Pro: https://developers.openai.com/api/docs/models/gpt-5.2-pro.md — medium/high/xhigh, **Responses-only**, Chat unsupported.
- Anthropic thinking matrix: https://platform.claude.com/docs/en/build-with-claude/thinking.md — manual/adaptive/off per model; sampling constraints termasuk API version baru.
- Manual thinking: https://platform.claude.com/docs/en/build-with-claude/extended-thinking.md — minimum budget 1024, budget di bawah max_tokens; Opus 4.5 satu-satunya extended-only dengan effort.
- Anthropic effort: https://platform.claude.com/docs/en/build-with-claude/effort.md — low/medium/high/xhigh/max availability berbeda; **4.6 mendukung max tetapi tidak xhigh**; Opus 4.5 tidak mendukung max.
- Adaptive steering: https://platform.claude.com/docs/en/build-with-claude/thinking-steering-and-cost — max_tokens hard cap, effort soft guidance.
- Always-thinking migration: https://platform.claude.com/docs/en/models/opus-5-5/migration-guide — disabled/manual ditolak; omit sampling parameters.
- Ollama compatibility: https://docs.ollama.com/api/openai-compatibility — reasoning_effort none untuk off; xhigh/ultra aliases; `/api/show` untuk metadata model.

Catatan API version: source request_headers yang dibaca masih `anthropic-version: 2023-06-01`; mempertahankan temperature pada legacy thinking-off masih valid untuk versi ini. Jika pengguna mengoverride header menjadi `2026-10-21`, docs menyatakan temperature/top_p/top_k ditolak pada semua model; adapter perlu membaca effective API version atau omit sampling global. Header tidak saya ubah.

## File dan gangguan tooling

Diubah: `src-tauri/src/agent/provider.rs` hanya payload/functions/helpers sebelum lock. Dibuat: `src-tauri/src/agent/provider_payload_audit_tests.rs`, laporan ini. Tidak ada commit, perubahan structs/event/SSE disengaja, schema config, atau source unrelated oleh subagent ini.

Tool patch pertama timeout 420s dan tidak mengubah target (dibuktikan RED tetap). Edit payload berikutnya memakai Python replacement exact setelah baca current file; meski area berbeda, read-modify-write whole file berpotensi menimpa edit parent, sehingga lock eksklusif memang wajib. Parent harus memeriksa diff final gabungan; saya tidak menulis provider.rs lagi setelah lock.

## Evaluasi deliverable

| Aspek | Skor | Evidence/gap |
|---|---|---|
| Akurasi | 4/5 | Empat RED→GREEN nyata dan docs resmi; acceptance adaptive belum dieksekusi. |
| Kelengkapan | 2/5 | P03 capability adaptation masih residual karena exclusive write lock. |
| Kejelasan | 4/5 | Status verified vs belum dijalankan dibedakan; laporan lebih panjang demi handoff lock. |
| Actionability | 4/5 | Module declaration, exact test command, mapping matrix tersedia; parent masih perlu implementasi. |
| Keringkasan | 3/5 | Banyak caveat endpoint/model; perlu ringkasan parent sebelum user-facing completion. |

Perbaikan tertinggi: parent integrasikan P03 capability gate lalu jalankan seluruh delapan tests; setelah itu reject/dokumentasikan Responses-only resmi dan lakukan request capture/live smoke dengan kredensial parent. Tidak ada live API/Ollama request pada task ini. Skill global tidak dibuat karena scope write dibatasi file task ini.
