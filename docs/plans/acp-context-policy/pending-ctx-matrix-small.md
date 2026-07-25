# S-CTX-MATRIX — Harness conformance matrix

## Outcome

Record observed ACP and native-context behavior for supported harnesses so
defaults remain evidence-led and no private system prompt is guessed from a
model's prose.

## Work

1. Capture initialize/session capability facts for each supported harness:
   embedded resources, resource links, session config options, and profile/mode
   option availability.
2. Run the same small workspace through Cursor and Mistral using a prompt that
   asks the model to identify visible client context. Treat the result as UX
   evidence only; capture the daemon's actual outgoing ACP blocks separately.
3. Record whether the harness natively exposes cwd, Git, inventory, tools, and
   its own system rules. Mark unknown rather than inferring.
4. Recommend only explicit policy defaults/overrides from recorded evidence.

## Acceptance

- Matrix distinguishes protocol capability, daemon-sent context, and
  harness-native observations.
- No code path assumes native workspace context based on `embeddedContext`.
- Matrix is updated when a harness/ACP version changes materially.

