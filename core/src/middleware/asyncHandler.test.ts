import { describe, it, expect, vi } from 'vitest';
import type { Request, Response, NextFunction } from 'express';
import { asyncHandler } from './asyncHandler.js';

describe('asyncHandler', () => {
  it('should call the wrapped function and pass arguments to it', async () => {
    const fn = vi.fn().mockResolvedValue('result');
    const handler = asyncHandler(fn);

    const req = {} as Request;
    const res = {} as Response;
    const next = vi.fn() as NextFunction;

    await handler(req, res, next);

    expect(fn).toHaveBeenCalledWith(req, res, next);
    expect(next).not.toHaveBeenCalled();
  });

  it('should catch rejected promises and call next with the error', async () => {
    const error = new Error('test error');
    const fn = vi.fn().mockRejectedValue(error);
    const handler = asyncHandler(fn);

    const req = {} as Request;
    const res = {} as Response;
    const next = vi.fn() as NextFunction;

    await handler(req, res, next);

    expect(fn).toHaveBeenCalledWith(req, res, next);
    expect(next).toHaveBeenCalledWith(error);
  });

  it('should catch synchronously thrown errors and call next with the error', async () => {
    const error = new Error('sync error');

    const asyncFn = vi.fn().mockImplementation(async () => {
      throw error;
    });

    const handler = asyncHandler(asyncFn);

    const req = {} as Request;
    const res = {} as Response;
    const next = vi.fn() as NextFunction;

    await handler(req, res, next);

    expect(asyncFn).toHaveBeenCalledWith(req, res, next);
    expect(next).toHaveBeenCalledWith(error);
  });
});
