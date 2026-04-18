import pg from 'pg';
const { Pool } = pg;

const pool = new Pool({
  connectionString: "postgresql://sera_user:sera_pass@localhost:5432/sera_db"
});

async function main() {
  try {
    const tableCheck = await pool.query("SELECT * FROM information_schema.tables WHERE table_name = 'thought_events'");
    console.log("Table exists:", tableCheck.rowCount > 0);
    
    if (tableCheck.rowCount > 0) {
      const columns = await pool.query("SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'thought_events'");
      console.log("Columns:", columns.rows);
    }
  } catch (err) {
    console.error("Query failed:", err.message);
  } finally {
    await pool.end();
  }
}

main();
