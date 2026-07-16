---
name: mermaid-diagram
model: sonnet
effort: low
description: Generate Mermaid diagrams to visualize data flow, architecture, or state machines. Use for debugging, documentation, or understanding complex systems.
user-invocable: true
disable-model-invocation: true
argument-hint: [optional: specific flow or component to diagram]
allowed-tools:
  - Read
  - Glob
  - Grep
  - Write
  - Agent
---

# Mermaid Diagram Generator

You have been asked to generate a Mermaid diagram. Follow these steps.

## Step 1: Determine Diagram Type

Ask the user what they need (or infer from context):

1. **Data flow** — How data moves through the system (request → processing → response)
2. **Architecture** — System components and their relationships
3. **Sequence** — Step-by-step interaction between components for a specific operation
4. **State machine** — States and transitions for a stateful entity
5. **Entity relationship** — Database schema and relationships

## Step 2: Explore the Code

Spin up parallel `explorer` agents based on the diagram type. Use the custom `explorer` agent (defined in `.claude/agents/`), never the built-in `Explore` type -- the built-in loads every MCP tool schema and blows the context window.

- **Data flow / Sequence:** "Trace the complete path of <specific operation> from entry point to final output, including middleware, services, and data stores. WHY: We need every hop in the chain to draw an accurate flow diagram."
- **Architecture:** "Map all top-level modules, their public interfaces, and which modules depend on which. WHY: We need the dependency graph to draw an accurate architecture diagram."
- **State machine:** "Find all state transitions for <entity>, including guards and side effects. WHY: We need every valid state transition to draw a complete state diagram."
- **ER diagram:** "Find all data models/schemas, their fields, and relationships (foreign keys, joins, references). WHY: We need the complete data model to draw an accurate ER diagram."

## Step 3: Generate the Diagram

Produce the Mermaid diagram in a fenced code block. Guidelines:

- Keep labels concise (3-5 words per node)
- Use subgraphs to group related components
- Include error paths and edge cases, not just happy path
- Add notes for non-obvious connections

## Step 4: Save the Diagram

Save to `docs/diagrams/<descriptive-name>.md` with:

1. A title and one-sentence description
2. The Mermaid diagram
3. A legend explaining any abbreviations or conventions
4. Date generated (for staleness tracking)

If `docs/diagrams/` does not exist, create it.

## Step 5: Offer Follow-up

After presenting the diagram, offer:

1. "Want me to trace a specific path through this diagram in more detail?"
2. "Want me to add error/failure paths?"
3. "Want me to generate a sequence diagram for a specific operation in this architecture?"
