### Single Description for Header Requirements

```text
UI Requirements: A dark-themed, VS Code-style right sidebar AI chat interface. It must include a fixed top navigation bar with dynamic, overflow-truncating tabs (featuring hover-to-reveal close buttons and a blue active-tab indicator) and a tightly spaced right-aligned icon menu (New Tab, History, Settings). The main scrollable chat area must feature right-aligned user bubbles and a left-aligned, multi-step agent lifecycle trace. This trace requires borderless, inline collapsible accordions for "Thinking" and "Tool Execution" (with localized scrolling for long outputs and left-border indentation), and standard text responses. The bottom-anchored input panel contains a text area and a toolbar with a file attach button, two context selector dropdowns, and a primary blue circular send button with an up-arrow.

```

---

### Developer Implementation Guide

**Overview**
The UI is a dark-themed, vertical panel designed to sit docked on the right side of an IDE (like VS Code). It uses a standard flexbox column layout to manage three main sections: a fixed top header, a scrollable chat history area in the middle, and a fixed input panel at the bottom.

**1. Top Header (Navigation & Tabs)**

* **Layout:** A fixed-height flex row with space-between alignment. A bottom border separates it from the chat area.
* **Tabs (Left side):** * Rendered as a horizontal flex row.
* **Text Truncation:** Tab text must use an ellipsis for overflow and never wrap to a second line.
* **Separators:** Tabs do not have full outlines; they are only separated by a subtle 1px vertical right border.
* **Active State:** The currently selected tab has a distinct blue top border (indicator line) and a slightly lighter background color than the inactive tabs.
* **Close Button:** An 'x' button inside each tab is hidden by default and only becomes visible when the user hovers over that specific tab. Small padding to keep tabs compact.


* **Action Menu (Right side):**
* A very tightly grouped row of three minimalist icons: a Plus (New Tab), a Clock/Circle (History), and a Three-Dot Menu (Context/Settings).
* The buttons have no backgrounds until hovered.



**2. Chat History Workspace (Middle)**

* **Layout:** A flex column that takes up all remaining vertical space. It must be scrollable (`overflow-y: auto`) with inner padding.
* **User Messages:** * Flex-aligned to the right.
* The text is encapsulated in a distinct chat bubble with a slightly lighter background than the main app.
* The border radius is rounded on all corners except the bottom-right, which is sharper to indicate the message origin.


* **Agent Messages (Left-aligned trace):**
* The agent's response is a sequential list of elements stacked vertically with tight spacing.
* **Thinking Block:** A collapsible accordion (defaults to closed). It has no outer borders. The summary line shows a small, monochrome brain icon. When expanded, the inner content is indented with a left border line, has a darker code-editor background, and displays italicized text.
* **Tool Execution Block:** A second collapsible accordion (defaults to closed). The summary line shows a monochrome gear icon. When expanded, the inner container uses the same left-border indent and darker background as the thinking block. It contains uppercase labels for "Command" and "Output", followed by monospaced terminal text. *Crucial:* This inner container must have a hard max-height (e.g., 250px) and its own vertical scrollbar so massive code outputs do not ruin the chat layout.
* **Final Text:** Standard paragraph text sitting directly below the collapsibles, completely unstyled and without borders.



**3. Input Panel (Bottom)**

Generally small padding to keep it compact without shrinking the text. 

* **Layout:** Anchored to the bottom of the sidebar with a top border.
* **Input Wrapper:** A container holding the text area and controls. It has rounded corners, a distinct border, and a slightly lighter dark background.
* **Text Area:** Sits at the top of the wrapper. It has no borders, a transparent background, and spans the full width.
* **Bottom Toolbar:** A flex row sitting below the text area, divided by a faint top border line.
* **Left Controls:** A tightly spaced row containing an icon button (Plus sign for attachments) and two standard `<select>` dropdown menus (one for the Model, one for Context/Profile). These elements look like small, bordered pill buttons.
* **Right Control (Send):** A distinct circular button pushed to the far right. It has a solid blue background and contains a simple white upward-pointing arrow.