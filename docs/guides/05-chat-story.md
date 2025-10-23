# Chat Flow Explained

ui/components/ChatApp.tsx orchestrates the Ink chat experience. Understanding the call flow helps you extend the palette or troubleshoot responses.

1. **User input** – typing /start-session --auto triggers handleSend, which recognises the slash command and tokenises arguments.
2. **Backend bridge** – BackendBridge.executeCommand('start-session', ['--auto'], { stream: false }) spawns ackend/main.py with the command. STDOUT/ERR are captured without streaming to avoid clobbering the chat UI.
3. **Python CLI** – ackend/main.py invokes cli (Click) with the provided command. The session tracker spins up and logs the invocation via log_cli_command.
4. **Chat updates** – ppendMessage records three entries:
   - You: /start-session --auto
   - System: Running context start-session --auto
   - Assistant: Session tracker started in background

All subsequent output (errors, status updates) is appended as additional assistant or system messages, keeping the conversation chronological. Adjacent commands like /session status or /bundle follow the exact same pipeline.

Because the palette wraps the real CLI, any new command you add to ackend/context_engine/cli.py automatically becomes available in chat.
