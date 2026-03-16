export class CentrifugoService {
  private static apiUrl = process.env.CENTRIFUGO_API_URL || 'http://centrifugo:8000/api';
  private static apiKey = process.env.CENTRIFUGO_API_KEY || 'sera-api-key';

  static async publish(channel: string, data: any): Promise<void> {
    try {
      const response = await fetch(this.apiUrl, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `apikey ${this.apiKey}`
        },
        body: JSON.stringify({
          method: 'publish',
          params: {
            channel,
            data
          }
        })
      });

      if (!response.ok) {
        const text = await response.text();
        console.error(`[CentrifugoService] Error publishing to ${channel}: ${response.status} - ${text}`);
      }
    } catch (error) {
      console.error(`[CentrifugoService] Failed to reach Centrifugo:`, error);
    }
  }
}
