# Cargo Cleaner

[cargo-clean-all](https://crates.io/crates/cargo-clean-all)にインスパイアされたTUI製のcargoのtargetファイルの削除ツールです。

TUI画面でcargo projectを選択して一括でtargetディレクトリの削除(cargo clean相当)を実行することができます。

# インストール方法

## cargo install

```bash
cargo install cargo-cleaner
```

# 使い方

## シンプルな使い方

```bash
cargo cleaner
```

この方法で起動すると、cargo-cleanerはHOMEディレクトリ以下の全てのディレクトリを対象に、targetディレクトリが正のサイズを持つCargoプロジェクトを探しに行きます。

## key-bind

| key     | description       |
|---------|-------------------|
| `h`     | ヘルプの表示            |
| `j` or ↓ | 下に移動              |
| `k` or ↑ | 上に移動              |
| `g`     | リストの先頭に移動         |
| `G`     | リストの末尾に移動         |
| `SPACE`  | カーソルのあるファイルを選択/解除 |
| `v`     | 自動選択モードに切り替える     |
| `V`     | 自動選択解除モードに切り替える   |
| `ESC`   | モードの解除            |
| `d`     | 選択したファイルを削除       |
| `q`     | 終了                |


## dry-run

```bash
cargo cleaner --dry-run
```

dry-runを指定すると、実際に削除を行わずに、削除対象となるファイルを表示します。

## Specify the directory

```bash
cargo cleaner -r <directory>
```

-rオプションを指定すると、指定したディレクトリ以下の全てのディレクトリを対象に、targetディレクトリが正のサイズを持つCargoプロジェクトを探しに行きます。
