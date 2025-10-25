import React from 'react';
import { Box, Text } from 'ink';
import SelectInput, { Item as SelectItem } from 'ink-select-input';

type PaletteItem = {
  label: string;
  command: string;
  args?: string[];
};

interface CommandPaletteProps {
  onSelect: (item: PaletteItem) => void;
  disabled?: boolean;
}

const paletteItems: PaletteItem[] = [
  { label: 'Initialize project (/init)', command: 'init' },
  { label: 'Start session auto (/start-session --auto)', command: 'start-session', args: ['--auto'] },
  { label: 'Stop session (/stop-session)', command: 'stop-session' },
  { label: 'Session save (/session save)', command: 'session', args: ['save'] },
  { label: 'Session status (/session status)', command: 'session', args: ['status'] },
  { label: 'Generate summary (/summary -m ai)', command: 'summary', args: ['-m', 'ai'] },
  { label: 'Compress context (/compress)', command: 'compress' },
  { label: 'Create bundle (/bundle)', command: 'bundle' },
  { label: 'Show config (/config show)', command: 'config', args: ['show'] },
  { label: 'Baseline list files (/baseline list)', command: 'baseline', args: ['list'] },
  { label: 'Baseline add files (/baseline add)', command: 'baseline', args: ['add'] },
  { label: 'Baseline review (/baseline review)', command: 'baseline', args: ['review'] },
  { label: 'Baseline auto generate (/baseline auto)', command: 'baseline', args: ['auto'] }
];

const selectItems: SelectItem<PaletteItem>[] = paletteItems.map((item) => ({
  label: item.label,
  value: item,
  key: item.label
}));

export default function CommandPalette({ onSelect, disabled = false }: CommandPaletteProps) {
  if (disabled) {
    return (
      <Box flexDirection="column">
        <Text color="yellow">Executing command...</Text>
      </Box>
    );
  }

  return (
    <Box flexDirection="column">
      <Text color="magenta">Command Palette</Text>
      <SelectInput
        items={selectItems}
        onSelect={(item) => onSelect(item.value)}
      />
      <Text color="gray">Use arrow keys + Enter, or type in the chat below with leading /.</Text>
    </Box>
  );
}
