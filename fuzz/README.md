# fuzz

nightly + cargo-fuzz 専用(ルートのワークスペースには含めない)。

```
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --locked
cd fuzz
cargo +nightly fuzz run policy_json     # ポリシー JSON 読込
cargo +nightly fuzz run classify_input  # オムニボックス入力の判定
cargo +nightly fuzz run decide_request  # ブロック判断
```

Windows(MSVC)では ASan ランタイム DLL(`clang_rt.asan_dynamic-x86_64.dll`)が PATH に必要。
Visual Studio の `VC\Tools\MSVC\<ver>\bin\Hostx64\x64` を PATH に追加する。
