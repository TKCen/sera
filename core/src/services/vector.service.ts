import { QdrantClient } from '@qdrant/js-client-rest';
import { Logger } from '../lib/logger.js';

const logger = new Logger('VectorService');

export interface VectorPoint {
  id: string | number;
  vector: number[];
  payload: any;
}

export class VectorService {
  private client: QdrantClient;
  private collectionName: string;

  constructor(collectionName: string = 'codebase') {
    this.collectionName = collectionName;
    this.client = new QdrantClient({
      url: process.env.QDRANT_URL || 'http://localhost:6333',
    });
  }

  public async ensureCollection(vectorSize: number) {
    const collections = await this.client.getCollections();
    const exists = collections.collections.some((c) => c.name === this.collectionName);

    if (!exists) {
      await this.client.createCollection(this.collectionName, {
        vectors: {
          size: vectorSize,
          distance: 'Cosine',
        },
      });
      logger.info(`Collection ${this.collectionName} created.`);
    }
  }

  public async upsertPoints(points: VectorPoint[]) {
    await this.client.upsert(this.collectionName, {
      wait: true,
      points: points.map((p) => ({
        id: p.id,
        vector: p.vector,
        payload: p.payload,
      })),
    });
  }

  public async search(vector: number[], limit: number = 5) {
    return await this.client.search(this.collectionName, {
      vector,
      limit,
      with_payload: true,
    });
  }
}
