> **Temporary fork.** `habeas-rig` is a copy of [`rig-bedrock`](https://crates.io/crates/rig-bedrock) 0.44.0, kept only so that [Habeas](https://github.com/nikomatsakis/habeas-codicem) can use it until the fix lands upstream. Don't depend on it; use `rig-bedrock`.
>
> **The one change:** upstream refuses to place a prompt-cache checkpoint once a reasoning turn is in the conversation, saying Bedrock rejects it. Bedrock accepts such checkpoints for Claude models (verified with `us.anthropic.claude-sonnet-5-5`, where the next request reads the checkpoint back), so this fork always places it. See `src/request.rs`. Habeas imports it under the name `rig_bedrock`, and this crate will be retired once upstream accepts the fix.

## Rig-Bedrock
This companion crate integrates AWS Bedrock as model provider with Rig.

## Usage

Add the companion crate to your `Cargo.toml`, along with the rig-core crate:

```toml
[dependencies]
rig-bedrock = "0.4.5"
rig-core = "0.36.0"
```

You can also run `cargo add rig-bedrock rig-core` to add the most recent versions of the dependencies to your project.

See the [`/examples`](./examples) folder for usage examples.

Make sure to have AWS credentials env vars loaded before starting client such as:
```shell
export AWS_DEFAULT_REGION=us-east-1
export AWS_SECRET_ACCESS_KEY=.......
export AWS_ACCESS_KEY_ID=......
```
