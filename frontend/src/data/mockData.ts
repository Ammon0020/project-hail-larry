import type {
  FileTreeNode,
  Agent,
  Session,
  PairedDevice,
  AppEvent,
} from '@/types'

/**
 * Mock data — shapes match what the Go daemon sends over WebSocket.
 * In production, these arrive as JSON via WebSocket events.
 * See Blueprint sections 5, 11, 13, 19.
 */

/** File tree (Blueprint Sec 13 — workspace management). */
export const mockFileTree: FileTreeNode[] = [
  {
    name: 'src',
    type: 'folder',
    expanded: true,
    icon: 'folder-open',
    iconColor: 'text-blue-400',
    children: [
      {
        name: 'routes',
        type: 'folder',
        expanded: true,
        icon: 'folder-open',
        iconColor: 'text-blue-400',
        children: [
          { name: 'index.js', type: 'file', icon: 'file-code', iconColor: 'text-yellow-400' },
          { name: 'auth.js', type: 'file', icon: 'file-code', iconColor: 'text-yellow-400' },
        ],
      },
      { name: 'server.js', type: 'file', icon: 'file-code', iconColor: 'text-yellow-400', active: true, unsaved: true, revision: 3 },
      { name: 'db.js', type: 'file', icon: 'file-code', iconColor: 'text-yellow-400' },
    ],
  },
  { name: 'tests', type: 'folder', expanded: false, icon: 'folder', iconColor: 'text-blue-400' },
  { name: 'package.json', type: 'file', icon: 'file-text', iconColor: 'text-gray-400' },
  { name: 'README.md', type: 'file', icon: 'file-text', iconColor: 'text-gray-400' },
]

/** Registered agents (Blueprint Sec 5 — agent registration). */
export const mockAgents: Agent[] = [
  {
    id: 'claude-code',
    name: 'Claude Code',
    models: [
      { id: 'claude-3.5-sonnet', name: 'Sonnet 3.5' },
      { id: 'claude-3-opus', name: 'Opus 3' },
      { id: 'claude-3-haiku', name: 'Haiku 3' },
    ],
  },
  {
    id: 'codex',
    name: 'Codex CLI',
    models: [
      { id: 'gpt-4o', name: 'GPT-4o' },
      { id: 'gpt-4-turbo', name: 'GPT-4 Turbo' },
    ],
  },
  {
    id: 'gemini',
    name: 'Gemini CLI',
    models: [
      { id: 'gemini-1.5-pro', name: 'Gemini 1.5 Pro' },
      { id: 'gemini-1.5-flash', name: 'Gemini 1.5 Flash' },
    ],
  },
  {
    id: 'cursor',
    name: 'Cursor CLI',
    models: [
      { id: 'cursor-small', name: 'Cursor Small' },
    ],
  },
]

/** Chat sessions (Blueprint Sec 4 — session, Sec 10 — lifecycle). */
export const mockSessions: Session[] = [
  { id: 's1', name: 'Server Setup',    time: '2m ago',    status: 'running',   active: true },
  { id: 's2', name: 'Auth refactor',   time: '1h ago',    status: 'completed' },
  { id: 's3', name: 'DB migration',    time: '3h ago',    status: 'completed' },
  { id: 's4', name: 'Fix CORS issue',  time: 'Yesterday', status: 'completed' },
  { id: 's5', name: 'Deploy script',   time: '2d ago',    status: 'failed' },
  { id: 's6', name: 'Refactor utils',  time: '3d ago',    status: 'archived' },
]

/** Paired devices (Blueprint Sec 19 — device pairing). */
export const mockDevices: PairedDevice[] = [
  { id: 'd1', name: 'iPhone',  icon: 'smartphone', pairedAt: '2024-01-15' },
  { id: 'd2', name: 'MacBook', icon: 'laptop',     pairedAt: '2024-01-10' },
]

/** Initial event stream (Blueprint Sec 11 — event system). */
export const mockEvents: AppEvent[] = [
  {
    type: 'PromptSubmitted',
    sessionId: 's1',
    role: 'user',
    content: 'Add error handling middleware to server.js and make sure the routes are properly imported.',
  },
  {
    type: 'ResponseStarted',
    sessionId: 's1',
    role: 'agent',
    content: "I'll add error handling middleware and verify the route imports. Let me check the current file first.",
  },
  {
    type: 'ToolCompleted',
    sessionId: 's1',
    tool: 'edit_file',
    target: 'server.js',
    summary: 'Added error handler at line 17-21',
  },
  {
    type: 'PermissionRequested',
    sessionId: 's1',
    tool: 'shell',
    command: 'npm test',
    options: ['allow_once', 'allow_session', 'allow_always', 'deny'],
  },
  {
    type: 'StreamUpdate',
    sessionId: 's1',
    role: 'agent',
    content: 'Running tests...',
    streaming: true,
  },
]

/** Mock code content for the editor (replaces hardcoded HTML in mockup). */
export const mockCodeContent = `import express from 'express';
import { connect } from './db.js';

const app = express();
const port = process.env.PORT || 3000;

// Middleware
app.use(express.json());
app.use(await connect());

// Routes
app.use('/api', await import('./routes'));

app.listen(port, () => {
  console.log(\`Server running on port \${port}\`);
});

// Unsaved edit — added error handler
app.use((err, req, res, next) => {
  console.error(err.stack);
  res.status(500).send('Something broke!');
});
`
