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
  queueCount?: number;
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
  queueCount = 0,
}: ChatInputBarProps) {
  return (
    <form
      className="flex items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        handleSend();
      }}
    >
      <div className="relative flex-1">
        <textarea
          ref={inputRef}
          aria-label="Chat input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={!selectedAgent}
          placeholder={
            streaming
              ? 'Type next message… (queued until response completes)'
              : selectedAgent
                ? 'Message agent… (Enter to send, Shift+Enter for newline)'
                : 'Select an agent above'
          }
          rows={1}
          className="sera-input w-full resize-none min-h-[38px] max-h-32 overflow-y-auto"
          style={{ height: 'auto' }}
          onInput={(e) => {
            const el = e.currentTarget;
            el.style.height = 'auto';
            el.style.height = `${Math.min(el.scrollHeight, 128)}px`;
          }}
        />
        {queueCount > 0 && (
          <span className="absolute right-2 top-1 text-[10px] font-medium text-sera-accent bg-sera-accent/15 rounded-full px-1.5 py-0.5">
            {queueCount} queued
          </span>
        )}
      </div>
      {streaming && onCancel ? (
        <button
          type="button"
          onClick={onCancel}
          className="flex-shrink-0 h-[38px] w-[38px] rounded-lg bg-sera-error/80 text-white flex items-center justify-center hover:bg-sera-error transition-all"
          title="Stop generating"
          aria-label="Stop generating"
        >
          <StopCircle size={14} />
        </button>
      ) : null}
      <button
        type="submit"
        disabled={!input.trim() || !selectedAgent}
        className="flex-shrink-0 h-[38px] w-[38px] rounded-lg bg-sera-accent text-sera-bg flex items-center justify-center disabled:opacity-40 disabled:cursor-not-allowed hover:brightness-110 transition-all"
        aria-label="Send message"
      >
        {streaming ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
      </button>
    </form>
  );
}
