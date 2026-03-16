export class Logger {
  private component: string;

  constructor(component: string) {
    this.component = component;
  }

  info(...args: any[]): void {
    console.log(`[${this.component}]`, ...args);
  }

  warn(...args: any[]): void {
    console.warn(`[${this.component}]`, ...args);
  }

  error(...args: any[]): void {
    console.error(`[${this.component}]`, ...args);
  }

  debug(...args: any[]): void {
    // console.debug(`[${this.component}]`, ...args);
  }
}
