# Habeas Codicem

Habeas Codicem is a simple requirements and examples system for agentic development. Requirements describe general behavior; examples show how it applies to concrete cases.

The Habeas Codicem workflow has three parts

1. You and your agent define **requirements** and **examples** in your markdown that ought to be satisfied.
2. The agent produces **evidence** explaining why it believes each requirement and example is satisfied and what source files it used to reach that conclusion. For an example, the agent explains how the tests cover its interesting aspects.
3. Habeas Codicem provides an **integration test** that checks that all the evidence is present and up-to-date (e.g., no input has changed).

## How Habeas Codicem keeps agents (relatively) honest

The key part is the integration test at the end. This test checks that all requirements and examples have evidence and that the inputs the agent used to reach its conclusion are still unchanged. It also checks that any test or part of the code tagged as pertaining to a definition appears in its evidence inputs.

Of course, this isn't *proof* -- the agent might have used faulty judgment or ignored necessary inputs. But it's a good step towards keeping your agent honest!

## Inspirations and alternatives

AI has taken requirement tracing out from the domain of safety critical systems (see e.g., the [NASA handbook](https://swehb.nasa.gov/spaces/SWEHBVD/pages/102695427/SWE-052+-+Bidirectional+Traceability) and into the mainstream. Habeas Codicem is but one project in this vein. Check out some of these others which are evolving quickly and have a bunch of cool ideas!

* Kari Wilhelm's [Anchor Book](https://github.com/anchor-book/anchor-book) uses **design anchors** to organize design decisions. Kari's anchors are based on her [Anchor Engineering](https://nappingtoheavymetal.com/series/anchor-engineering/) and go beyond simple requirements to document the design decisions that went into establishing the requirements as well as how they ought to be verified and tested.
* Faster-than-lime's [Tracey](https://github.com/bearcove/tracey) gives Markdown requirements stable IDs and links them to code and tests. It includes an MCP server and skills to help agents work with them.
* [GitHub's Spec Kit](https://github.com/github/spec-kit) and [Kiro's spec-driven-development approach](https://kiro.dev/docs/specs/) organize an agent's work around requirements, design, and tasks.
