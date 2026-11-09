# Bugfix round — wiring & responsive (2026-11-06)

## Bug yang ditemukan & diperbaiki

| # | Severity | Bug | Fix |
|---|---|---|---|
| 1 | P1 | `/review` dan `/reflect` tidak pernah diteruskan ke ChatCanvas. Fallback mengirim "/review" sebagai pesan chat ke LLM, sehingga fitur Self-Improvement Review mati di app nyata. | `App.tsx` meneruskan `onReviewSession={agent.reviewSession}` dan `isReviewing={agent.isReviewing}`. |
| 2 | P2 | Slash command terdaftar tapi handler tidak pernah diteruskan: `/compact`, `/compress`, `/save`, `/retry`, `/undo`, `/init`, `/title` semuanya no-op. | Meneruskan `onCompactSession`, `onUndoTurn`, `onInitAgentsMd`, `onExportSession` (dengan adapter argumen), `onRenameSession`, `currentSessionId`. |
| 3 | P2 | Pemilihan pemenang Council tidak melakukan apa pun (`onSelectCouncilWinner` tidak diteruskan). | Meneruskan `agent.selectCouncilWinner`. |
| 4 | P2 | Sidebar tidak bisa ditutup. `onToggleSidebar`/`sidebarOpen` tidak pernah diteruskan; pada viewport 375px sidebar 256px memakan 68% layar tanpa cara menutup. Tombol toggle tidak pernah dirender. | State `sidebarOpen` di App, toggle di header, Ctrl+B (sesuai title yang sudah ada), Escape, backdrop, dan auto-close saat memilih sesi pada viewport sempit. Mode `overlay` Sidebar yang sudah ada kini dipakai. |
| 5 | P2 | Toolbar header terpotong `overflow-hidden` pada 375px: tombol Artifacts dan More actions tidak terjangkau. | Title `max-w` dikurangi di base breakpoint, gap toolbar dan max-width model picker dikecilkan; seluruh tombol kini masuk viewport dan clickable. |

## Bukti

- Tes RED→GREEN: `src/App.test.tsx` (6 tes). Sebelum perbaikan keenam tes gagal dengan alasan yang benar (reviewSession tidak dipanggil, tombol toggle tidak ada, Ctrl+B tidak bekerja, backdrop tidak ada).
- Suite penuh: `npm test` **49 tes / 11 file lulus**; `tsc --noEmit` lulus; `npm run build` lulus.
- Browser asli (mock IPC terisolasi, dev server 127.0.0.1:1421):
  - 375px: kelima tombol header dalam viewport dan `elementFromPoint` mengembalikan tombol itu sendiri; tidak ada document overflow; composer dan tombol kirim dalam batas.
  - Drawer sempit: mulai tertutup, toggle membuka overlay `fixed` 256px + backdrop, backdrop menutup.
  - `/review` pada sesi kosong: menampilkan error "Complete a conversational turn before reviewing." tanpa invoke.
  - `/review` setelah turn selesai: `review_session` terinvoke dengan sessionId dan provider yang benar; `send_message` tidak terinvoke.

## Sisa temuan (belum diperbaiki, dilaporkan ke user)

- Modal yang sudah dibangun dan berTES tapi tidak pernah di-mount di `App.tsx`, sehingga slash command berikutnya masih mati: `/history` (SessionHistoryModal), `/tools` (ToolsInspectorModal), `/memory` (MemoryModal). Juga `SpotlightBar` dan `InteractiveTerminal` tidak dirender.
- Composer model menu (`settings`, `onSelectModel`, `onUpdateOverride`), indikator `subagents`/`activeSkills`, dan `onListSkills` tidak diteruskan ke ChatCanvas.
- `/init` memakai `alert()` browser native sebagai notifikasi.
