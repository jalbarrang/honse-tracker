# Agents

Honse tracker is a plugin for the Hachimi program for the "Honse Game" it's job is to allow users that are interested on it to keep track of data that happens behind the scenes of the game, and have values that you normally do by hand or using external tools in browser.

`honse-tracker` in general is a Rust workspace which it produces a DLL.

## Code Conventions

- Avoid formatting using PEP8, this is not a Python project.

## Comments

The modules here open with essays. That is fine for what a comment is good at
and dangerous for what it is not, so:

- Write down what the code cannot tell you: a struct's real layout in the
  game, a format another project owns, why a read is safe where it happens.
  Name the source and the build — "`il2cpp_classes.txt`, Global 2026-08-30" —
  so the next reader knows what to re-check rather than what to trust.
- Do not assert what other code does. "The light refresh covers this screen"
  reads as a guarantee, rots the moment that code moves, and is cheaper to
  verify than to write. Go and look instead.
- Describe what the code does, not what it was meant to do. A comment
  promising that fields "are asked for by name, so a rename is followed rather
  than misread" sat above a reader that resolved nothing and exported zeros.
- A claim you can check belongs in a test. The planner encoder agrees with
  torena-hub because a test pins codes the TypeScript encoder produced — not
  because a paragraph says they agree.
