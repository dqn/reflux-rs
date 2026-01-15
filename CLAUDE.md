# Reflux-RS

beatmania IIDX INFINITAS のスコアトラッカー。本家 [Reflux](https://github.com/olji/Reflux) (C#) の Rust 移植版。

## プロジェクト構成

```
crates/
├── reflux-core/    # コアライブラリ（ゲームロジック、メモリ読み取り、API）
└── reflux-cli/     # CLI アプリケーション
```

## 開発コマンド

```bash
cargo build          # ビルド
cargo test           # テスト実行
cargo run            # CLI 実行（Windows のみ動作）
```

## アーキテクチャ

### reflux-core モジュール構成

| モジュール | 役割 |
|-----------|------|
| `game/` | ゲームデータ構造（PlayData, Judge, Settings, etc.） |
| `memory/` | Windows プロセスメモリ読み取り |
| `network/` | リモートサーバー API、Kamaitachi 連携 |
| `storage/` | ローカルファイル保存（Tracker, Session, UnlockDb） |
| `stream/` | OBS 向けファイル出力 |
| `offset/` | メモリオフセット検索・管理 |
| `config/` | INI 設定ファイルパース |
| `reflux.rs` | メインアプリケーションロジック |

### 主要な型

- `PlayData` - プレイ結果データ
- `Judge` - 判定データ（PGreat, Great, etc.）
- `SongInfo` - 楽曲メタデータ
- `UnlockData` - アンロック状態
- `Tracker` - ベストスコア追跡
- `OffsetsCollection` - メモリオフセット集

## 参照資料

本家 C# 実装は `.agent/Reflux/` にあり。機能追加・バグ修正時に参照。

## リリース手順

1. Cargo.toml のバージョンを更新（reflux-core, reflux-cli 両方）
2. `git tag vX.Y.Z` でタグをつける
3. `git push --tags` で push

## 注意事項

- Windows 専用（INFINITAS のメモリを読み取るため）
- macOS/Linux ではビルドは通るがメモリ読み取り機能は動作しない
- Shift-JIS エンコーディング処理あり（日本語タイトル対応）

## 改善ロードマップ

### Phase 1: Critical (データ整合性・信頼性)

- [x] **ファイル更新のアトミック化** (`network/api.rs:141-161`)
  - `persist()` を先に実行し、成功後にアーカイブ処理を行う
  - 失敗時のデータロスを防止

- [ ] **オフセット検索の信頼性向上** (`offset/searcher.rs:703-752`)
  - 追加バリデーション（隣接メモリ領域の構造チェック）
  - 段階的検索範囲拡大（2MB → 10MB → 50MB → 300MB）
  - デバッグログで候補数を出力

### Phase 2: Warning (堅牢性・デバッグ性)

- [x] **リトライメカニズムの改善** (`reflux.rs:249-290`)
  - 指数バックオフの導入（50ms, 100ms, 200ms）

- [x] **非同期エラーの可視化** (`reflux.rs:428-442`)
  - 失敗したペイロードの概要をログに記録

- [x] **パースエラー処理の簡潔化** (`storage/tracker.rs:58-184`)
  - 最初の 10 件のみ詳細ログ、以降はカウントのみ

- [ ] **Shift-JIS デコードの柔軟化** (`memory/reader.rs:88-103`)
  - 部分的に正しい文字列を返すオプション追加

- [x] **設定パースエラーの警告** (`main.rs:315-327`)
  - カスタムタイプ ID パース失敗時に警告ログ出力

### Phase 3: Info (保守性・テスト)

- [ ] **テストカバレッジ向上** (`offset/searcher.rs`)
  - モックメモリリーダーを作成
  - エッジケースのテスト追加

- [x] **マジックナンバーの定数化** (`reflux.rs:1020` など)
  - 遅延時間に意図を示す定数名を付与

- [x] **リトライロジックの共通化** (`network/client.rs`)
  - `post_form()` と `get()` の重複を抽出

- [x] **INI パーサーのエラー改善** (`config/mod.rs`)
  - エラー型に行番号情報を追加

- [ ] **unwrap() の明確化**
  - `expect("reason")` に変更、または安全性コメント追加
