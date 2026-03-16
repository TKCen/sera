"use client";

import dynamic from 'next/dynamic';
import React from 'react';

// Use dynamic import with ssr: false to prevent Window is not defined error on server side
const MemoryGraph = dynamic(() => import('./MemoryGraph'), {
  ssr: false,
  loading: () => (
    <div className="w-full h-[600px] flex items-center justify-center border border-sera-border rounded-lg bg-[#0a0a0a]">
      <div className="animate-pulse flex flex-col items-center gap-4 text-sera-text-muted">
        <div className="w-8 h-8 rounded-full border-2 border-sera-primary border-t-transparent animate-spin"></div>
        Loading graph visualization...
      </div>
    </div>
  )
});

export default function MemoryGraphWrapper(props: any) {
  return <MemoryGraph {...props} />;
}
