---
name: agents-map
description: Build or refresh a hierarchical, token-optimized AGENTS.md directory map for this repo. Use when creating or updating repo-wide context maps.
argument-hint: ""
allowed-tools:
  - read
  - exec
  - find_file_by_name
  - grep
  - write
  - edit
  - todo_write
---

# AGENTS.md Directory Map

Analyze this codebase and build a hierarchical, token-optimized directory map system using `AGENTS.md` files to guide developer and AI agent workflows.

## Objective

Create a top-level `AGENTS.md` with a high-level filemap, and place nested `AGENTS.md` files in key subdirectories (e.g., app packages, backend services, or UI components) so agents can "lazy load" deep directory context on demand without flooding context windows.

## Instructions

1. **Analyze the Repository Structure**:
   - Inspect the codebase to identify key architectural boundaries, core modules, packages, and major subdirectories.

2. **Update / Create Root `AGENTS.md`**:
   - Add a `## File Map` section outlining the high-level architecture.
   - Group files by domain/package rather than listing every individual file.
   - Add explicit references pointing to subfolder maps, e.g.:
     `**apps/web/** - Web Client (See apps/web/AGENTS.md)`

3. **Create Subdirectory `AGENTS.md` Files**:
   - For each major sub-package or core directory, create a local `AGENTS.md` containing:
     - **Header**: Directory path and high-level responsibility summary.
     - **Module Map**: Breakdown of subfolders, key entry points, and primary components.
     - **Rules & Patterns**: Folder-specific constraints (e.g., "All DB queries must go through repository layer", "Pure functions only, no DOM/API imports").

4. **Formatting Constraints**:
   - **Token Efficiency**: Use concise bullet points and bold headers. Do not write lengthy paragraphs.
   - **Modularity**: Focus on responsibilities and boundaries ("where things live and why") rather than exhaustive file listings.
   - Keep files small, clean, and easily scannable.
