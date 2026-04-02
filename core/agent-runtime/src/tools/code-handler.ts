import { log } from '../logger.js';

export async function codeEval(
  code: string,
  language: 'javascript' | 'typescript' = 'javascript',
  timeout: number = 5000
): Promise<string> {
  const actualTimeout = Math.min(Math.max(timeout, 100), 30000);
  const start = Date.now();

  // Using Bun.eval for isolated JS execution if available,
  // or a restricted new Function() as a fallback.
  // Since we are running in Bun, let's use its capabilities.

  try {
    const stdout: string[] = [];
    const stderr: string[] = [];

    // Create a restricted global scope
    const sandbox = {
      console: {
        log: (...args: any[]) => stdout.push(args.map(a => String(a)).join(' ')),
        error: (...args: any[]) => stderr.push(args.map(a => String(a)).join(' ')),
        warn: (...args: any[]) => stderr.push(args.map(a => String(a)).join(' ')),
      },
      process: undefined,
      Buffer: undefined,
      require: undefined,
      setTimeout: setTimeout,
      setInterval: setInterval,
      fetch: undefined,
      Bun: undefined,
    };

    // We use a wrapper to capture the return value
    const wrappedCode = `
      (function(sandbox) {
        with(sandbox) {
          ${code}
        }
      })(sandbox)
    `;

    // Note: 'with' statement is normally discouraged but useful for simple sandboxing
    // of a provided object's properties.

    // In a real production environment, we'd use a more robust sandbox like 'vm2' or 'isolated-vm',
    // but those are not available in this environment. Bun's eval is also an option.

    let result: any;

    // Create a promise that rejects after timeout
    const timeoutPromise = new Promise((_, reject) =>
      setTimeout(() => reject(new Error(`Execution timed out after ${actualTimeout}ms`)), actualTimeout)
    );

    const executionPromise = (async () => {
       // Function constructor is slightly safer than eval as it doesn't capture local scope
       const fn = new Function('sandbox', `
         with(sandbox) {
           ${language === 'typescript' ? '// Typescript not natively supported in Function, treating as JS\n' : ''}
           return (async () => {
             ${code}
           })()
         }
       `);
       return await fn(sandbox);
    })();

    result = await Promise.race([executionPromise, timeoutPromise]);

    const elapsed = Date.now() - start;

    const output = {
      result,
      stdout: stdout.join('\n'),
      stderr: stderr.join('\n'),
      elapsedMs: elapsed
    };

    return JSON.stringify(output, null, 2);

  } catch (err) {
    return JSON.stringify({
      error: err instanceof Error ? err.message : String(err),
      elapsedMs: Date.now() - start
    }, null, 2);
  }
}
