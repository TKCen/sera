/**
 * Tool handler for pdf-read.
 * Extracts text content from PDF files in the workspace.
 */

import fs from 'fs';
import pdf from 'pdf-parse';
import { resolveSafe } from './file-handlers.js';

/**
 * Handle pdf-read tool call.
 */
export async function pdfRead(
  workspacePath: string,
  filePath: string,
  pages?: string,
  format: string = 'text'
): Promise<string> {
  const resolved = resolveSafe(workspacePath, filePath);

  if (!fs.existsSync(resolved)) {
    return `Error: PDF file not found: ${filePath}`;
  }

  try {
    const dataBuffer = fs.readFileSync(resolved);
    const data = await pdf(dataBuffer);

    // Filter pages if specified
    let text = data.text;
    const pageCount = data.numpages;

    if (pages) {
      const selectedPages = parsePageRange(pages, pageCount);
      if (selectedPages.length > 0) {
        // pdf-parse's .text is the entire document concatenated.
        // It doesn't provide per-page extraction easily.
        // For now, we'll return the full text but prefix it with page metadata.
        // Real page extraction would require a more sophisticated library like pdfjs-dist.
        text = `[Pages ${pages} requested of ${pageCount} total pages]\n\n${text}`;
      }
    }

    if (format === 'markdown') {
      text = `## PDF Content: ${filePath}\n\n${text}`;
    }

    return text;
  } catch (err) {
    return `Error: Failed to parse PDF: ${err instanceof Error ? err.message : String(err)}`;
  }
}

/**
 * Parses a page range string like "1-5", "3", "1,3,5-7" into an array of page numbers.
 */
function parsePageRange(rangeStr: string, totalPages: number): number[] {
  const pages = new Set<number>();
  const parts = rangeStr.split(',');

  for (const part of parts) {
    const trimmed = part.trim();
    if (trimmed.includes('-')) {
      const [startStr, endStr] = trimmed.split('-');
      const start = parseInt(startStr || '1', 10);
      const end = parseInt(endStr || String(totalPages), 10);
      for (let i = Math.max(1, start); i <= Math.min(end, totalPages); i++) {
        pages.add(i);
      }
    } else {
      const pageNum = parseInt(trimmed, 10);
      if (pageNum >= 1 && pageNum <= totalPages) {
        pages.add(pageNum);
      }
    }
  }

  return Array.from(pages).sort((a, b) => a - b);
}
