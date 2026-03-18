import { NextRequest, NextResponse } from 'next/server';

async function handleResponse(response: Response) {
  if (response.status === 204) {
    return new NextResponse(null, { status: 204 });
  }

  const contentType = response.headers.get('content-type');
  if (contentType && contentType.includes('application/json')) {
    const data = await response.json();
    return NextResponse.json(data, { status: response.status });
  } else {
    const text = await response.text();
    return new NextResponse(text, {
      status: response.status,
      headers: {
        'Content-Type': contentType || 'text/plain',
      },
    });
  }
}

function handleError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return NextResponse.json({ error: message }, { status: 500 });
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  const { path } = await params;
  const pathSegments = path.join('/');
  const searchParams = request.nextUrl.searchParams.toString();
  const url = `${process.env.CORE_API_URL || 'http://localhost:3001'}/api/${pathSegments}${searchParams ? `?${searchParams}` : ''}`;

  try {
    const response = await fetch(url, {
      method: 'GET',
    });

    return await handleResponse(response);
  } catch (error) {
    return handleError(error);
  }
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  const { path } = await params;
  const pathSegments = path.join('/');
  const searchParams = request.nextUrl.searchParams.toString();
  const url = `${process.env.CORE_API_URL || 'http://localhost:3001'}/api/${pathSegments}${searchParams ? `?${searchParams}` : ''}`;

  try {
    const contentType = request.headers.get('content-type');
    let body = null;
    if (contentType && contentType.includes('application/json')) {
      body = await request.text();
    }

    const response = await fetch(url, {
      method: 'POST',
      headers: body ? { 'Content-Type': 'application/json' } : {},
      body: body || undefined,
    });

    return await handleResponse(response);
  } catch (error) {
    return handleError(error);
  }
}

export async function PUT(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  const { path } = await params;
  const pathSegments = path.join('/');
  const searchParams = request.nextUrl.searchParams.toString();
  const url = `${process.env.CORE_API_URL || 'http://localhost:3001'}/api/${pathSegments}${searchParams ? `?${searchParams}` : ''}`;

  try {
    const contentType = request.headers.get('content-type');
    let body = null;
    if (contentType && contentType.includes('application/json')) {
      body = await request.text();
    }

    const response = await fetch(url, {
      method: 'PUT',
      headers: body ? { 'Content-Type': 'application/json' } : {},
      body: body || undefined,
    });

    return await handleResponse(response);
  } catch (error) {
    return handleError(error);
  }
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ path: string[] }> }
) {
  const { path } = await params;
  const pathSegments = path.join('/');
  const searchParams = request.nextUrl.searchParams.toString();
  const url = `${process.env.CORE_API_URL || 'http://localhost:3001'}/api/${pathSegments}${searchParams ? `?${searchParams}` : ''}`;

  try {
    const response = await fetch(url, {
      method: 'DELETE',
    });

    return await handleResponse(response);
  } catch (error) {
    return handleError(error);
  }
}

