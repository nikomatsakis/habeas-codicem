# Habeas Codicem

Habeas Codicem is a simple requirements system for agentic development. The goal is that you should be able to lay out *requirements* for how the code ought to work and then ignore the code itself.

The Habeas Codicem workflow has three parts

1. You and your agent define **requirements** in your markdown that ought to be satisfied.
2. The agent produces **evidence** explaining (a) why it believes each requirement is satisfied and (b) what source files it used to reach that conclusion.
3. Habeas Codicem provides an **integration test** that checks that all the evidence is present and up-to-date (e.g., no input has changed).

## How Habeas Codicem keeps agents (relatively) honest

The key part is the integration test at the end. This test checks that all requirements have evidence and that the inputs the agent used to reach its conclusion are still unchanged. It also checks that any test or part of the code that is tagged as pertaining to a requirement must in the list of inputs.

Of course, this isn't *proof* -- the agent might have used faulty judgment or ignored necessary inputs. But it's a good step towards keeping your agent honest!

## Inspirations and alternatives

AI has taken requirement tracing out from the domain of safety critical systems (see e.g., the [NASA handbook](https://swehb.nasa.gov/spaces/SWEHBVD/pages/102695427/SWE-052+-+Bidirectional+Traceability) and into the mainstream. Habeas Codicem is but one project in this vein. Check out some of these others which are evolving quickly and have a bunch of cool ideas!

* Kari Wilhelm's [Anchor Book](https://github.com/anchor-book/anchor-book) uses **design anchors** to organize design decisions. Kari's anchors are based on her [Anchor Engineering](https://nappingtoheavymetal.com/series/anchor-engineering/) and go beyond simple requirements to document the design decisions that went into establishing the requirements as well as how they ought to be verified and tested.
* Faster-than-lime's [Tracey](https://github.com/bearcove/tracey) gives Markdown requirements stable IDs and links them to code and tests. It includes an MCP server and skills to help agents work with them.
* [GitHub's Spec Kit](https://github.com/github/spec-kit) and [Kiro's spec-driven-development approach](https://kiro.dev/docs/specs/) organize an agent's work around requirements, design, and tasks.
