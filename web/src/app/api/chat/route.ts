import { NextResponse } from 'next/server';

export async function POST(req: Request) {
  try {
    const body = await req.json();
    const { message, id } = body;

    // Here you would normally forward the request to the backend service (e.g., sera-core)
    // For now, we simulate a delay and echo back if no real backend is connected.

    // Simulate thinking delay
    await new Promise(resolve => setTimeout(resolve, 2000));

    return NextResponse.json({
      content: `Received prompt: "${message}". System processing...`,
      id
    });
  } catch (error) {
    return NextResponse.json(
      { error: 'Failed to process chat request' },
      { status: 500 }
    );
  }
}
