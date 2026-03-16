import { config } from '../src/lib/config.js';

async function validateLLM() {
  const llmConfig = config.llm;

  console.log(`Configured Provider: ${llmConfig.provider}`);
  console.log(`Configured Base URL: ${llmConfig.baseUrl}`);
  console.log(`Configured Model: ${llmConfig.model}`);

  try {
    let url = `${llmConfig.baseUrl}/models`;

    if (llmConfig.provider === 'lm-studio') {
      // LM Studio's native REST API is at /api/v1/models
      // Parse the configured baseUrl to construct the correct native API endpoint
      try {
        const parsedUrl = new URL(llmConfig.baseUrl);
        // Replace /v1 path with /api/v1 to use native API
        if (parsedUrl.pathname === '/v1' || parsedUrl.pathname === '/v1/') {
          parsedUrl.pathname = '/api/v1/models';
        } else if (parsedUrl.pathname === '/api/v1' || parsedUrl.pathname === '/api/v1/') {
          parsedUrl.pathname = '/api/v1/models';
        } else {
          // If no specific path or different path, just append /api/v1/models
          parsedUrl.pathname = parsedUrl.pathname.replace(/\/+$/, '') + '/api/v1/models';
        }
        url = parsedUrl.toString();
      } catch (e) {
        // Fallback to string replacement if URL parsing fails
        if (llmConfig.baseUrl.endsWith('/v1') || llmConfig.baseUrl.endsWith('/v1/')) {
          url = llmConfig.baseUrl.replace(/\/v1\/?$/, '/api/v1/models');
        } else {
          url = `${llmConfig.baseUrl.replace(/\/+$/, '')}/api/v1/models`;
        }
      }
    }

    console.log(`Fetching models from: ${url}`);

    const response = await fetch(url, {
      method: 'GET',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${llmConfig.apiKey}`
      }
    });

    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }

    const data = await response.json();
    console.log('\nAvailable Models:');
    if (data && data.data && Array.isArray(data.data)) {
      data.data.forEach((model: any) => {
        console.log(`- ${model.id}`);
      });
    } else {
      console.log(JSON.stringify(data, null, 2));
    }

  } catch (error) {
    console.error('Failed to validate LLM connection:', error);
    process.exit(1);
  }
}

validateLLM();
