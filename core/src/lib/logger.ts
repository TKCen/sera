export class Logger {
  private component: string;

  constructor(component: string) {
    this.component = component;
  }

  info(...args: unknown[]): void {
    console.log(`[${this.component}]`, ...args);
  }

  warn(...args: unknown[]): void {
    console.warn(`[${this.component}]`, ...args);
  }

  error(...args: unknown[]): void {
    console.error(`[${this.component}]`, ...args);
  }

  debug(..._args: unknown[]): void {
    // console.debug(`[${this.component}]`, ..._args);
  }
}
