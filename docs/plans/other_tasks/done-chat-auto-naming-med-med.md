# Chat auto-naming

> **Status:** done | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — Agent Chat section

## Goal

Automatically generate a short, descriptive name for each chat session based
on its content, similar to how ChatGPT or other chat clients auto-title
conversations.

## Approach

- After the first user prompt (or first agent response), generate a short
  title (3-5 words) from the conversation content.
- Options: use the agent itself to generate a title (via a lightweight prompt),
  or use a heuristic (first N words of the first user message, truncated).
- The auto-name should be editable by the user.
- Avoid extra API calls if possible — prefer heuristic unless the agent
  supports a cheap title-generation method.

## Acceptance

- [x] New chats get an auto-generated name after first message exchange
- [x] Name is editable by the user
- [x] Name appears in chat list and session history
- [x] `make check` passes
