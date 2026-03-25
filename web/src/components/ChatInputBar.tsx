import React from 'react';
import { Loader2, Send, StopCircle } from 'lucide-react';

interface ChatInputBarProps {
  inputRef: React.RefObject<HTMLTextAreaElement | null>;
  input: string;
  setInput: (value: string) => void;
  handleKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  streaming: boolean;
  selectedAgent: string;
  handleSend: () => void;
  onCancel?: () => void;
}

export function ChatInputBar({
  inputRef,
  input,
  setInput,
  handleKeyDown,
  streaming,
  selectedAgent,
  handleSend,
  onCancel,
}: ChatInputBarProps) {
  return (
    <div className="flex items-end gap-2">
      <textarea
        ref={inputRef}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={streaming || !selectedAgent}
        placeholder={
          streaming
            ? 'Agent is responding…'
            : selectedAgent
              ? 'Message agent… (Enter to send, Shift+Enter for newline)'
              : 'Select an agent above'
        }
        rows={1}
        className="sera-input flex-1 resize-none min-h-[38px] max-h-32 overflow-y-auto"
        style={{ height: 'auto' }}
        onInput={(e) => {
          const el = e.currentTarget;
          el.style.height = 'auto';
          el.style.height = `${Math.min(el.scrollHeight, 128)}px`;
        }}
      />
      {streaming && onCancel ? (
        <button
          onClick={onCancel}
          className="flex-shrink-0 h-[38px] w-[38px] rounded-lg bg-sera-error/80 text-white flex items-center justify-center hover:bg-sera-error transition-all"
          title="Stop generating"
        >
          <StopCircle size={14} />
        </button>
      ) : (
        <button
          onClick={() => void handleSend()}
          disabled={streaming || !input.trim() || !selectedAgent}
          className="flex-shrink-0 h-[38px] w-[38px] rounded-lg bg-sera-accent text-sera-bg flex items-center justify-center disabled:opacity-40 disabled:cursor-not-allowed hover:brightness-110 transition-all"
        >
          {streaming ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
        </button>
      )}
    </div>
  );
}
