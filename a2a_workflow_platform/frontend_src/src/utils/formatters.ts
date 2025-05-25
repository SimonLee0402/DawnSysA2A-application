export function formatDateTime(isoString: string | Date): string {
  if (!isoString) return '';
  try {
    const date = new Date(isoString);
    // Basic format: YYYY-MM-DD HH:MM:SS
    // For more advanced formatting, a library like date-fns or moment.js could be used.
    const year = date.getFullYear();
    const month = (date.getMonth() + 1).toString().padStart(2, '0');
    const day = date.getDate().toString().padStart(2, '0');
    const hours = date.getHours().toString().padStart(2, '0');
    const minutes = date.getMinutes().toString().padStart(2, '0');
    const seconds = date.getSeconds().toString().padStart(2, '0');
    return `${year}-${month}-${day} ${hours}:${minutes}:${seconds}`;
  } catch (error) {
    console.error("Error formatting date:", isoString, error);
    return String(isoString); // Fallback to original string if formatting fails
  }
}


