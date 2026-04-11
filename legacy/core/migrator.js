import pgmigrate from 'node-pg-migrate';
import pg from 'pg';

const databaseUrl = 'postgresql://sera_user:sera_pass@sera-db:5432/sera_db';

async function run() {
  const client = new pg.Client({
    connectionString: databaseUrl,
  });
  await client.connect();
  
  try {
    await pgmigrate({
      dir: 'src/db/migrations',
      dbClient: client,
      direction: 'up',
      count: Infinity,
      migrationsTable: 'pgmigrations',
    });
    console.log('Migrations completed successfully');
  } catch (err) {
    console.error('Migration failed:', err);
    process.exit(1);
  } finally {
    await client.end();
  }
}

run();
