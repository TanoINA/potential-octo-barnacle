# Hook fixes — hasil slice integrasi

Source dilepas ke parent setelah focused test terakhir. Tidak ada commit, full suite, build frontend/native yang dijalankan worker ini. Audit awal memuat **15 temuan (6 H, 8 M, 1 L)** di `layer-state-review.md`; ini bukan klaim semuanya selesai.

## File milik worker yang diubah
- `src/hooks/useAgent.ts`
- `src/hooks/useAgent.reflection.test.tsx`
- NEW `src/hooks/useAgentLiveSession.ts`
- Dokumen `docs/audit/layer-state-review.md`, laporan ini.

App/components/native/config/shared types bukan milik worker dan tidak diedit worker ini. Shared schema `workspaceCwd`, `providerMessages`, `provider_state` diberikan parent.

## Selesai dalam slice ini
- **LIFE-01 / ST-06:** New/Switch tidak lagi abort. Registry LiveSession per session menyimpan run/message identity, data, buffers/RAF, subagents, skills, artifact selection metadata, failure/stop state. Event `{sessionId,runId}` diroute ke origin meskipun offscreen; done flush+save origin. Review event offscreen juga update snapshot origin.
- Normal/council request A/B menyertakan `workspaceCwd` dari snapshot origin. Native canonicalize/binding dimiliki parent; hook regression memastikan request memiliki cwd A meski B/new ditampilkan. `newSession(): string` **tetap sinkron** mengembalikan ID baru, cocok kontrak slash `/new` dan mocks.
- Start continuation/catch terikat LiveSession+run origin; kegagalan A tidak null/reset RB. Explicit Stop sebelum initial save selesai mencegah start RA. Clear/Delete boleh abort; offscreen delete menarget run ID origin dan menghapus map entry.
- **ST-02 initial:** await load_session dan set_workspace saat restore sebelum loading false; send ditolak saat initialization. Navigation pending ref menahan send selama switch, finally hanya reset untuk owner generation; New dan return-to-current membatalkan pending intent.
- **ST-03 sebagian:** edit/retry memakai detach yang flush per-session buffer, dan edit after-await menolak resend jika navigation/session berubah.
- **Provider state:** handler `provider_state` mengganti (bukan append) assistant.providerMessages origin; final save+reload mempertahankan nested thinking/signature/tool payload. Native roundtrip/replay verification tetap parent.
- **IFR-01 H:** compact commit lewat mutateData agar authoritative LiveSession ikut berubah. Regression compact→updateRunConfig tidak menghidupkan history lama.
- **IFR-02 H:** live snapshot yang pernah dimutasi ditandai authoritative dan tetap menang atas disk setelah done meskipun final save masih antre. Snapshot loaded yang belum dimutasi boleh re-read disk (menjaga delayed-review/load test). Regression return dari empty view sebelum final save selesai.

## Bukti verifikasi

Terakhir:
```
npm test -- src/hooks/useAgent.reflection.test.tsx
Test Files 1 passed (1)
Tests 15 passed (15)
```
Exit focused test 0; dijalankan lagi sesudah kedua IFR H diperbaiki.

`npx tsc --noEmit` sempat exit 0 pada slice 9/11/12/13 test. Pemeriksaan terakhir exit 2 **hanya pada file di luar scope yang sedang diedit paralel**:
- `src/components/ExecutionTrace.test.tsx`: unused within
- `src/components/ExecutionTrace.tsx`: unused expandedDetails/traceId/toggleDetails dan missing expandedTerminals/toggleTerminal.
Tidak ada diagnostics hook pada output terakhir. Parent perlu gate ulang setelah component worker selesai.

RED nyata yang diamati: cancel-on-switch lifecycle (1 fail/7 pass), init loading (1 fail/8 pass), Stop-before-start (1 fail/11 pass), edit→New continuation (1 fail/12 pass). Provider_state dan kedua IFR regressions ditambahkan saat bounded integration; yang terakhir sudah diuji GREEN, bukan diklaim RED-before-fix.

## Residual tepat — jangan klaim semua 15 temuan closed

1. **F02/ST-11:** live map belum eviction/budget; buffers RAF belum timer fallback hidden-window dan total byte cap; belum throttled streaming checkpoint. Done flush+save teruji, crash sebelum done tetap dapat kehilangan output terakhir. Parent perlu bounded completed-cache eviction hanya setelah persistence sukses, jangan evict authoritative unsaved state.
2. **ST-09 resetPartial:** belum diubah. Notice masih reset content/reasoning tetapi bukan segments/tools/checkpoint per attempt. Tool-only retry phantom pending dan valid earlier-turn reasoning belum diselesaikan. Council backend tidak emit retry notice saat audit; jangan membuat klaim retry Council palsu.
3. **IFR-03 M:** exposed raw `setActiveArtifactId` belum menulis live.artifactId; next publish dapat membatalkan selection. Loaded liveSession default artifactId null. Fix callback parent/lanjutan diperlukan.
4. **IFR-04 M:** prestart Stop masih mempublish stopRequested=false ketika langsung idle; UI queue latch dapat auto-resume. Hook test hanya memastikan initial run tidak launch; integrasi nyata Canvas queue Stop membutuhkan persistent pause intent.
5. **ST-04/07/08/14:** tombstone/revision coordinator seluruh save/rename/pin/delete/compact, settings concurrent rollback/fetch stale snapshot, rollback/edit save-failure recovery belum menyeluruh. Compaction view authoritative sudah fix IFR-01, bukan semua mutation-ordering race.
6. NavigationPending ditutup dengan finally ownership; belum dedicated deferred switch/send regression baru (initial loading regression tersedia). Clear saat pending navigation juga perlu cek ownership reset/invalidation; New reset sudah eksplisit.
7. Await invoke send vs abort registry begin gap tetap perlu native cancellation tombstone/handshake untuk Stop setelah invoke dispatched tetapi sebelum backend begin. Hook pre-save cancellation sudah teruji.
8. Session-operation/review lock masih global (dapat menahan sesi B ketika review A offscreen); UI global toast bisa menampilkan error A saat view B. Stop subagent/skill immediate React setters belum seluruhnya mengubah live arrays sampai event done; audit lanjutan.
9. Listener readiness/rejection, modal/memory/shortcut, speech errors, queue options/identity, git/workspace global request ordering dimiliki parent/components atau tetap residual laporan awal; tidak disentuh melebar.
10. Authoritative snapshot cache berarti native mutation noncurrent (mis. rename/pin) harus synchronize map entry juga; saat ini beberapa callbacks hanya update viewed session. Parent perlu field-level metadata revision dan cache invalidation, bukan blindly disk overwrite.

Laporan awal tetap berstatus audit-statis historis; klaim implementasi/tes yang sah hanya dalam dokumen ini. Tidak menyentuh `docs/audit-evidence-tools-2026-11-08.md`.
