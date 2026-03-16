import { pipeline } from '@xenova/transformers';

export class EmbeddingService {
  private static instance: EmbeddingService;
  private extractor: any;

  private constructor() {}

  public static getInstance(): EmbeddingService {
    if (!EmbeddingService.instance) {
      EmbeddingService.instance = new EmbeddingService();
    }
    return EmbeddingService.instance;
  }

  private async getExtractor() {
    if (!this.extractor) {
      this.extractor = await pipeline('feature-extraction', 'Xenova/all-MiniLM-L6-v2');
    }
    return this.extractor;
  }

  public async generateEmbedding(text: string): Promise<number[]> {
    const extractor = await this.getExtractor();
    const output = await extractor(text, { pooling: 'mean', normalize: true });
    return Array.from(output.data) as number[];
  }
}
