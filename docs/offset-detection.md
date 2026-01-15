# オフセット自動検出の挑戦記録

## 概要

beatmania IIDX INFINITAS のスコアトラッカー reflux-rs において、ゲーム更新時に必要な offsets.txt の手動更新を自動化しようと試みた記録。

## 実装したもの（v0.1.19 時点）

### CLI オプション
- `--debug-offsets`: 詳細なデバッグログ出力
- `--dump-offsets`: オフセット情報を JSON 出力
- `--force-interactive`: 自動検出をスキップしてインタラクティブ検索を強制

### 自動検出の構造（search_all）

```
Phase 1: 静的パターン検索（高信頼度）
  - SongList: "P2D:J:B:A:" パターン（最後のマッチ）
  - UnlockData: (1000, 1, 462) パターン
  - DataMap: (0x7FFFF, 0) パターン

Phase 2: 初期状態パターン検索（中信頼度）
  - JudgeData: 72バイトゼロ + STATE_MARKER検証
  - PlaySettings: 1i32 マーカー + 範囲検証

Phase 3: 固定オフセット計算
  - PlayData = PlaySettings + 0x2B0
  - CurrentSong = JudgeData + 0x1F4

Phase 4-5: 検証
```

## 発見した問題

### 1. 固定オフセットはバージョンで変わる

**過去バージョン（2024〜2025前半）:**
```
playData - playSettings = 0x2B0 (688)
currentSong - judgeData = 0x1F4 (500)
```

**2025122400（最新）:**
```
playData - playSettings = 0x2C0 (704)  ← 変わった！
currentSong - judgeData = 0x1E4 (484)  ← 変わった！
```

### 2. 「最後のマッチ」戦略が常に正しいわけではない

自動検出と正しい値の比較:
```
SongList:
  自動検出: 0x14353AF38（最後のマッチ）
  正しい値: 0x14315A380（最後ではない）
```

C# のコメント「最初の2つは2016-build参照」が常に正しいわけではない。

### 3. JudgeData の72バイトゼロパターンは誤検出しやすい

- 2MB 範囲で 1,766,666 個の候補が見つかる
- STATE_MARKER 検証でも絞り切れない
- DataMap との距離検証（1-15MB）も不正確

### 4. 自動検出が「成功」しても間違っている

検出は完了するが、以下の問題が発生:
- `Loaded 1 songs`（本来は数千曲）
- `capacity overflow` でクラッシュ

## データ比較

### 自動検出 vs インタラクティブ（2025122400）

| オフセット | 自動検出（❌） | インタラクティブ（✓） |
|-----------|--------------|---------------------|
| song_list | 0x14353AF38 | 0x14315A380 |
| judge_data | 0x143CD1966 | 0x14280C00C |
| play_settings | 0x143E7F488 | 0x14255F124 |
| play_data | 0x143E7F738 | 0x14255F3E4 |
| current_song | 0x143CD1B5A | 0x14280C1F0 |

### オフセット関係の変遷

| バージョン | playData - playSettings | currentSong - judgeData |
|-----------|------------------------|------------------------|
| 2024042400 | 0x2B0 | 0x1F4 |
| 2024052200 | 0x2B0 | 0x1F4 |
| 2025082000 | 0x2B0 | 0x1F4 |
| 2025101500 | 0x2B0 | 0x1F4 |
| **2025122400** | **0x2C0** | **0x1E4** |

## 学んだこと

1. **固定オフセットは信用できない** - ゲーム更新で構造体サイズが変わる
2. **パターン検索は誤検出が多い** - 特にゼロパターンは大量にマッチする
3. **「最後のマッチ」は万能ではない** - バージョンによって変わる
4. **検出成功 ≠ 正しい検出** - 検証が不十分だと間違った値で進んでしまう
5. **診断ツールは必須** - `--dump-offsets` なしでは問題特定が困難

## 次のセッションへの提案

### アプローチ A: バージョン別ヒントファイル

```json
{
  "P2D:J:B:A:2025122400": {
    "play_data_from_play_settings": 704,
    "current_song_from_judge_data": 484
  },
  "P2D:J:B:A:2025101500": {
    "play_data_from_play_settings": 688,
    "current_song_from_judge_data": 500
  }
}
```

新バージョンではインタラクティブ検索 → 結果をヒントファイルに追加。

### アプローチ B: 検証強化

1. SongList 検出後、実際に曲を読み込んでみて数千曲あるか確認
2. 数曲しか読めない場合は検出失敗として別のマッチを試す
3. 全マッチを記録し、検証に通るものを選択

### アプローチ C: インタラクティブ検索のみに戻す

自動検出の信頼性が低いため、インタラクティブ検索のみをサポートし、結果を offsets.txt に保存するシンプルな構成に戻す。

## 関連ファイル

- `crates/reflux-core/src/offset/searcher.rs` - 検索ロジック
- `crates/reflux-core/src/offset/dump.rs` - 診断ダンプ
- `crates/reflux-cli/src/main.rs` - CLI エントリポイント
- 参照: https://github.com/olji/Reflux/commits/master/Reflux/offsets.txt
