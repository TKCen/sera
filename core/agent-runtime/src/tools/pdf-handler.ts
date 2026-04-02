import fs from 'fs';
import pdf from 'pdf-parse';
import { resolveSafe } from './file-handlers.js';

export async function pdfRead(
  workspacePath: string,
  filePath: string,
  pages?: string,
  format: 'text' | 'markdown' = 'text'
): Promise<string> {
  const resolved = resolveSafe(workspacePath, filePath);
  if (!fs.existsSync(resolved)) {
    return `Error: PDF file not found: ${filePath}`;
  }

  try {
    const dataBuffer = fs.readFileSync(resolved);

    // pdf-parse doesn't support page ranges directly in its base call
    // but we can parse the whole thing and then slice, or use a custom pagerender.
    // For simplicity and to satisfy the requirement of page markers, we'll parse it all.

    const options = {
      pagerender: (pageData: any) => {
        return pageData.getTextContent().then((textContent: any) => {
          let lastY, text = '';
          for (let item of textContent.items) {
            if (lastY == item.transform[5] || !lastY){
              text += item.str;
            } else {
              text += '\n' + item.str;
            }
            lastY = item.transform[5];
          }
          return `\n--- PAGE ${pageData.pageIndex + 1} ---\n${text}`;
        });
      }
    };

    const data = await pdf(dataBuffer, options);
    let text = data.text;

    if (pages) {
      const pageSet = parsePageRange(pages, data.numpages);
      const allPages = text.split(/\n--- PAGE \d+ ---\n/).filter(p => p.trim().length > 0);

      const filtered = Array.from(pageSet)
        .sort((a, b) => a - b)
        .filter(p => p >= 1 && p <= allPages.length)
        .map(p => `--- PAGE ${p} ---\n${allPages[p-1]}`);

      text = filtered.join('\n\n');
    }

    if (text.trim().length === 0) {
      return "Warning: No text extracted from PDF. This might be a scanned image PDF. Try image-view instead.";
    }

    return text;
  } catch (err) {
    return `Error: Failed to parse PDF: ${err instanceof Error ? err.message : String(err)}`;
  }
}

function parsePageRange(rangeStr: string, maxPages: number): Set<number> {
  const pages = new Set<number>();
  const parts = rangeStr.split(',');

  for (const part of parts) {
    if (part.includes('-')) {
      const [start, end] = part.split('-').map(p => parseInt(p.trim()));
      const s = isNaN(start) ? 1 : start;
      const e = isNaN(end) ? maxPages : end;
      for (let i = s; i <= e; i++) {
        pages.add(i);
      }
    } else {
      const p = parseInt(part.trim());
      if (!isNaN(p)) pages.add(p);
    }
  }

  return pages;
}
