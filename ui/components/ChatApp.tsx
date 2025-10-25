import React, { useCallback, useMemo, useState } from 'react';
import { Box, Text } from 'ink';
import ChatPanel, { ChatMessage, ChatRole } from './ChatPanel';
import CommandPalette from './CommandPalette';

type BackendResult = {
  stdout: string;
  stderr: string;
  code: number;
};

const BackendBridge: any = require('../lib/backend-bridge');

function createMessage(role: ChatRole, text: string): ChatMessage {
  return {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    role,
    text: text.trim()
  };
}

function parseCliJson(stdout: string) {
  const lines = stdout.trim().split('\n').filter(Boolean);
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    const candidate = lines[i];
    try {
      return JSON.parse(candidate);
    } catch {
      continue;
    }
  }
  return null;
}

function tokenize(input: string): string[] {
  const tokens: string[] = [];
  const regex = /"([^"]+)"|'([^']+)'|(\S+)/g;
  let match: RegExpExecArray | null;

  while ((match = regex.exec(input)) !== null) {
    tokens.push(match[1] ?? match[2] ?? match[3]);
  }

  return tokens;
}

export default function ChatApp() {
  const backend = useMemo(() => new BackendBridge(), []);
  const [messages, setMessages] = useState<ChatMessage[]>([
    createMessage('system', 'Palette ready. Select a command or type /<command>.')
  ]);
  const [busy, setBusy] = useState(false);

  const appendMessage = useCallback((role: ChatRole, text: string) => {
    setMessages((prev) => [...prev, createMessage(role, text)]);
  }, []);

  const runCommand = useCallback(
    async (command: string, args: string[] = []) => {
      setBusy(true);
      const commandLine = ['context', command, ...args].join(' ').trim();
      appendMessage('system', `Running ${commandLine}`);

      try {
        const result: BackendResult = await backend.executeCommand(command, args, { stream: false });
        const payload = parseCliJson(result.stdout);

        if (payload && payload.success) {
          const response =
            payload.summary ||
            payload.output ||
            payload.message ||
            'Command completed successfully.';
          appendMessage('assistant', response.trim());
        } else if (payload && !payload.success) {
          appendMessage('assistant', `Command failed: ${payload.error || 'Unknown error'}`);
        } else if (result.stdout.trim()) {
          appendMessage('assistant', result.stdout.trim());
        }

        if (result.stderr && result.stderr.trim()) {
          appendMessage('system', result.stderr.trim());
        }
      } catch (error: any) {
        const errorMessage = error.message || 'Unknown error occurred';
        appendMessage('assistant', `Command error: ${errorMessage}`);
      } finally {
        setBusy(false);
      }
    },
    [appendMessage, backend]
  );

  const handlePaletteSelect = useCallback(
    (item: { command: string; args?: string[] }) => {
      const args = item.args ?? [];
      const rendered = `/${[item.command, ...args].join(' ').trim()}`.replace(/\s+/g, ' ').trim();
      appendMessage('user', rendered);
      runCommand(item.command, args);
    },
    [appendMessage, runCommand]
  );

  const handleSend = useCallback(
    (value: string) => {
      if (!value.trim()) {
        return;
      }

      appendMessage('user', value);

      if (value.startsWith('/')) {
        const tokens = tokenize(value.slice(1));
        if (tokens.length === 0) {
          appendMessage('system', 'Please provide a command after /.');
          return;
        }

        const [command, ...args] = tokens;
        runCommand(command, args);
        return;
      }

      appendMessage(
        'system',
        'Treating message as AI summary request. Use "/" prefix to run explicit CLI commands.'
      );
      runCommand('summary', ['-m', 'ai']);
    },
    [appendMessage, runCommand]
  );

  return (
    <Box flexDirection="column" gap={1}>
      <Box flexDirection="column">
        <Text color="cyan">Interactive Context Engine</Text>
        <Text color="gray">
          Palette up top, chat below. Prefix commands with "/" (e.g. /start-session --auto).
        </Text>
      </Box>
      <CommandPalette onSelect={handlePaletteSelect} disabled={busy} />
      <ChatPanel messages={messages} onSend={handleSend} busy={busy} />
    </Box>
  );
}
