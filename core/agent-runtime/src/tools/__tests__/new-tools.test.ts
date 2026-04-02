import { describe, it, expect, vi, beforeEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import { imageView } from '../image-handler.js';
import { pdfRead } from '../pdf-handler.js';
import { codeEval } from '../code-eval-handler.js';
import { httpRequest } from '../http-handler.js';

vi.mock('fs');

describe('New Built-in Tools', () => {
  const workspacePath = '/workspace';

  beforeEach(() => {
    vi.resetAllMocks();
  });

  describe('image-view', () => {
    it('should return a vision request marker for supported images', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.statSync).mockReturnValue({ isFile: () => true, size: 1024 } as any);
      vi.mocked(fs.readFileSync).mockReturnValue(Buffer.from('fake-image-data'));

      const result = imageView(workspacePath, 'test.png', 'what is this?');
      const parsed = JSON.parse(result);

      expect(parsed.__sera_vision_request__).toBe(true);
      expect(parsed.dataUrl).toContain('data:image/png;base64,');
      expect(parsed.prompt).toBe('what is this?');
      expect(parsed.path).toBe('test.png');
    });

    it('should return an error for non-existent files', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false);
      const result = imageView(workspacePath, 'missing.png');
      expect(result).toContain('Error: Image file not found');
    });

    it('should return an error for unsupported formats', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true);
      vi.mocked(fs.statSync).mockReturnValue({ isFile: () => true, size: 1024 } as any);
      const result = imageView(workspacePath, 'test.txt');
      expect(result).toContain('Error: Unsupported image format');
    });
  });

  describe('code-eval', () => {
    it('should execute simple JS and return result', async () => {
      const result = await codeEval('1 + 1');
      expect(result).toContain('Result: 2');
    });

    it('should capture console.log output', async () => {
      const result = await codeEval('console.log("hello"); "done"');
      expect(result).toContain('Stdout:\nhello');
      expect(result).toContain('Result: "done"');
    });

    it('should enforce timeout', async () => {
      const result = await codeEval('while(true);', 'javascript', 100);
      expect(result).toContain('Execution failed');
      expect(result).toContain('Script execution timed out');
    });
  });

  describe('http-request', () => {
    it('should block non-http protocols', async () => {
      const result = await httpRequest('file:///etc/passwd');
      expect(result).toContain('Error: Only http and https URLs are allowed');
    });

    it('should block private addresses', async () => {
      const result = await httpRequest('http://192.168.1.1');
      expect(result).toContain('Error: Fetching private/local addresses is not allowed');
    });
  });
});
