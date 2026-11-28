# trust-but-verify

`trust-but-verify` is a Rust SDK for building checks that can explain which
source inputs support a result. Habeas Codicem uses it to connect written
requirements to the code and tests that satisfy them.

Your application defines the criteria to check and the project data they can
inspect. The SDK tracks the inputs each check reads and fingerprints them so
you can tell when stored evidence needs to be refreshed. Checks can inspect
source outlines, search project files, or request a typed assessment from a
configured model.

```sh
cargo add trust-but-verify
```

[API documentation](https://docs.rs/trust-but-verify) · [Repository](https://github.com/nikomatsakis/habeas-codicem)
