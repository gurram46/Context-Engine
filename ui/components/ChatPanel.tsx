import React, { useState } from 'react';
import { Box, Text } from 'ink';
import TextInput from 'ink-text-input';

export type ChatRole = 'user' | 'system' | 'assistant';

export interface ChatMessage {
  id: string;
  role: ChatRole;
  text: string;
}

interface ChatPanelProps {
  messages: ChatMessage[];
  onSend: (value: string) => void;
  busy?: boolean;
}

const rolePrefix: Record<ChatRole, string> = {
  user: 'You',
  assistant: 'AI',
  system: 'System'
};

export default function ChatPanel({ messages, onSend, busy = false }: ChatPanelProps) {
  const [input, setInput] = useState('');

  const submit = () => {
    const value = input.trim();
    if (!value || busy) {
      return;
    }

    onSend(value);
    setInput('');
  };

  return (
    <Box
      flexDirection="column"
      borderStyle="round"
      borderColor="blue"
      paddingX={1}
      paddingY={0}
      width="100%"
    >
      <Box flexDirection="column" flexGrow={1} marginBottom={1}>
        {messages.length === 0 ? (
          <Text color="gray">Type /command or a prompt to get started.</Text>
        ) : (
          messages.slice(-12).map((message) => (
            <Text key={message.id} wrap="wrap">
              <Text color={message.role === 'user' ? 'green' : message.role === 'assistant' ? 'cyan' : 'yellow'}>
                {rolePrefix[message.role]}:
              </Text>{' '}
              {message.text}
            </Text>
          ))
        )}
      </Box>

      <Box>
        <Text color="cyan">{busy ? '...' : '>'} </Text>
        <TextInput
          value={input}
          onChange={setInput}
          placeholder="Type a command or message..."
          onSubmit={submit}
        />
      </Box>
    </Box>
  );
}
