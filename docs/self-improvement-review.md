# Self-Improvement Review

## Perilaku

Fitur ini opt-in. Buka **Settings → Agent defaults → Agent Intelligence / Reflection**, lalu aktifkan **Enable automatic self-improvement**. Pengaturan `self_improvement_enabled` bernilai `false` untuk konfigurasi baru maupun konfigurasi lama yang belum memiliki field ini.

Setelah run utama berhasil dan menggunakan beberapa turn provider atau tools, evaluator terpisah meninjau snapshot turn tersebut. Run gagal, dibatalkan, mencapai batas turn, dan jawaban sederhana tidak memicu review otomatis. Tanpa workspace aktif, review otomatis dilewati; perintah manual menampilkan error yang jelas.

`/review` dan `/reflect` meninjau turn percakapan terakhir yang sudah selesai, termasuk saat mode otomatis nonaktif. Keduanya tidak menambahkan pesan user atau memulai ulang run utama. Provider dan model mengikuti override sesi yang sedang aktif. Satu review per sesi dan jumlah review bersamaan dibatasi.

## Arsitektur

- `AgentSettings` dan `AgentRunRequest`: opt-in yang eksplisit pada kontrak Rust/TypeScript.
- `agent/reflection.rs`: completion tambahan tanpa tools, prompt khusus, parsing `NO_OP` atau JSON terstruktur, serta penulisan lokal tervalidasi.
- `agent/runner.rs`: pemicu setelah run sukses, snapshot turn, koordinasi background, dan event.
- `agent/storage.rs`: hasil review disimpan pada pesan asal; snapshot frontend yang lebih lama tidak boleh menghapus hasil review native.
- `agent/skills.rs`: `.aether/skills` workspace ikut ditemukan dan diprioritaskan dalam katalog skill.
- `useAgent.ts`: menerima event terpisah dari stream utama dan mencocokkan `session_id` serta `run_id`, termasuk sesudah `done` atau ketika turn berikutnya sedang berjalan.
- `SelfImprovementBadge.tsx`: label inline di bawah jawaban assistant; klik membuka isi mentah `SKILL.md` dalam inspector yang mendukung keyboard.

## Penyimpanan dan batas keamanan

Skill ditulis ke `<workspace>/.aether/skills/<name>/SKILL.md`. Tidak ada fallback diam-diam ke direktori global. Nama skill harus berupa slug yang aman; traversal, device path Windows, symlink/reparse point, dan target di luar workspace ditolak. File menggunakan YAML frontmatter agar dapat dibaca skill loader. Review tidak mengubah `AGENTS.md` secara otomatis; pelajaran disimpan sebagai skill lokal agar instruksi proyek yang sudah ada tidak ditimpa.

Review memakai request tambahan ke provider aktif, jadi ada biaya dan pengiriman konteks turn sesuai provider yang dipilih. Evaluator tidak mengeksekusi tools atau perintah dari transkrip. Hasil provider tetap dianggap input tak tepercaya dan divalidasi sebelum ditulis.

Event `agent:self-improvement` menggunakan payload berikut:

```typescript
{
  run_id: string;
  session_id: string;
  action: 'skill_created' | 'skill_updated' | 'no_action';
  skill_name?: string;
  summary?: string;
  workspace_cwd?: string; // Workspace asal, bukan workspace yang baru dipilih.
}
```

`agent:self-improvement-error` melaporkan kegagalan review tanpa mengubah keberhasilan run utama. Inspector hanya membaca skill yang terhubung dengan review tersimpan di sesi itu; nama skill bukan jalur file arbitrer.

## Struktur dan batas implementasi

`reflection.rs` menampung evaluator dan penulisan skill secara terpisah. `runner.rs`, `storage.rs`, `useAgent.ts`, dan `ChatCanvas.tsx` sudah berupa modul besar sebelum fitur ini; perubahan lifecycle sengaja tetap berada pada pemilik state yang ada, bukan memindahkan arsitektur di tengah perubahan lokal lain. File tes reflection dipisah dari implementasi; ukurannya berasal dari fixture HTTP nyata dan matriks keamanan filesystem.

Penulisan menggunakan resolver, inspeksi reparse, lock proses, snapshot compare, serta atomic rename. Ini mencegah escape pada path/link yang diperiksa dan kehilangan update antar-review dalam satu proses, tetapi bukan sandbox terhadap proses lokal berprivilege setara yang sengaja mengganti parent tepat di sela syscall. Hardening handle-relative lintas platform memerlukan pekerjaan tersendiri.

## Verifikasi

Tes Rust mencakup parsing, `NO_OP`, pemicu opt-in, batas path, penulisan/update, provider mock HTTP lokal, snapshot turn, dan persistence. Tes Vitest mencakup badge/inspector, settings, perintah slash, serta hasil background yang datang setelah run selesai atau ketika run baru dimulai.

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm test
npm run build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Completion provider diuji memakai server HTTP lokal deterministik, bukan akun atau API berbayar. Hasil pengujian akhir dicatat terpisah pada laporan implementasi.
