import { toast as sonnerToast } from 'sonner';
import type { ExternalToast } from 'sonner';

export const toast = {
  success: (message: string, options?: ExternalToast) => sonnerToast.success(message, options),
  error: (message: string, options?: ExternalToast) => sonnerToast.error(message, options),
  info: (message: string, options?: ExternalToast) => sonnerToast.info(message, options),
  warning: (message: string, options?: ExternalToast) => sonnerToast.warning(message, options),
  message: (message: string, options?: ExternalToast) => sonnerToast(message, options),
};
