import api from './api';
import { Document, DocumentCreate, DocumentType, InvoiceCreate, ReturnInvoiceCreate, PurchaseOrderCreate, BatchConfirmRequest } from '@/types/document';
import { PaginatedResponse, SearchParams } from '@/types/api';

/** Отримує правильний ендпоінт для типу документа */
function getEndpointForType(type: DocumentType): string {
  switch (type) {
    case 'invoice': return '/invoices';
    case 'return_invoice': return '/return-invoices';
    case 'purchase_order': return '/purchase-orders';
    case 'transfer': return '/transfers';
    case 'write_off': return '/write-offs';
    case 'inventory': return '/inventory';
    default: return '/documents';
  }
}

export const documentService = {
  async getDocuments(params?: SearchParams & { document_type?: DocumentType }): Promise<PaginatedResponse<Document>> {
    const response = await api.get<PaginatedResponse<Document>>('/documents', { params });
    return response.data;
  },

  async getDocument(id: string): Promise<Document> {
    const response = await api.get<Document>(`/documents/${id}`);
    return response.data;
  },

  async createDocument(data: DocumentCreate): Promise<Document> {
    // Залежно від типу документа використовуємо відповідний ендпоінт
    switch (data.document_type) {
      case 'invoice': {
        const invoiceData = data as InvoiceCreate;
        const response = await api.post<Document>('/invoices', {
          number: invoiceData.number,
          supplier_id: invoiceData.supplier_id,
          invoice_date: invoiceData.invoice_date,
          payment_method: invoiceData.payment_method || undefined,
          is_fiscal: invoiceData.is_fiscal,
          notes: invoiceData.notes,
          items: invoiceData.items.map(item => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            cost_price: item.cost_price ?? item.price,
            markup_percent: item.markup_percent ?? 0,
            total: item.total ?? (item.quantity * item.price),
          })),
        });
        return response.data;
      }
      case 'return_invoice': {
        const returnData = data as ReturnInvoiceCreate;
        const response = await api.post<Document>('/return-invoices', {
          // Якщо number не вказано, бекенд згенерує автоматично
          number: returnData.number || undefined,
          supplier_id: returnData.supplier_id,
          return_date: returnData.return_date,
          return_action: returnData.return_action || 'deduct_from_debt',
          is_fiscal: returnData.is_fiscal,
          notes: returnData.notes,
          items: returnData.items.map(item => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            cost_price: item.cost_price,
            total: item.total ?? (item.quantity * item.price),
          })),
          // Опціональна прив'язка до прибуткової накладної
          source_invoice_id: returnData.source_invoice_id || undefined,
        });
        return response.data;
      }
      case 'purchase_order': {
        const orderData = data as PurchaseOrderCreate;
        const response = await api.post<Document>('/purchase-orders', {
          number: orderData.number || undefined,
          supplier_id: orderData.supplier_id,
          order_date: orderData.order_date,
          expected_date: orderData.expected_date || undefined,
          is_fiscal: orderData.is_fiscal,
          notes: orderData.notes,
          items: orderData.items.map(item => ({
            product_id: item.product_id,
            quantity: item.quantity,
            price: item.price,
            total: item.total ?? (item.quantity * item.price),
          })),
        });
        return response.data;
      }
      case 'inventory': {
        const invData = data as any;
        const response = await api.post<Document>('/inventory', {
          number: invData.number,
          location: invData.location,
          inventory_date: invData.inventory_date,
          notes: invData.notes,
          items: invData.items,
        });
        return response.data;
      }
      case 'transfer':
        throw new Error('Transfer creation not implemented yet');
      case 'write_off':
        throw new Error('Write-off creation not implemented yet');
      default:
        throw new Error(`Unknown document type: ${data.document_type}`);
    }
  },

  async updateDocument(id: string, data: Partial<DocumentCreate>): Promise<Document> {
    const response = await api.put<Document>(`/documents/${id}`, data);
    return response.data;
  },

  async confirmDocument(id: string, documentType?: DocumentType): Promise<Document> {
    const endpoint = documentType ? `${getEndpointForType(documentType)}/${id}/confirm` : `/documents/${id}/confirm`;
    // Надсилаємо статус "confirmed" в тілі запиту, як очікує бекенд
    const response = await api.post<Document>(endpoint, { status: 'confirmed' });
    return response.data;
  },

  async cancelDocument(id: string, documentType?: DocumentType): Promise<Document> {
    const endpoint = documentType ? `${getEndpointForType(documentType)}/${id}/cancel` : `/documents/${id}/cancel`;
    // Надсилаємо статус "cancelled" в тілі запиту
    const response = await api.post<Document>(endpoint, { status: 'cancelled' });
    return response.data;
  },

  async deleteDocument(id: string, documentType: DocumentType): Promise<void> {
    const endpoint = getEndpointForType(documentType);
    await api.delete(`${endpoint}/${id}`);
  },

  /** Масове підтвердження документів */
  async batchConfirm(items: Array<{ id: string; document_type: DocumentType }>): Promise<void> {
    await api.post('/documents/batch-confirm', { items });
  },

  /** Копіювання документа */
  async copyDocument(id: string, documentType: DocumentType): Promise<Document> {
    const response = await api.post<Document>(`/documents/${id}/copy?document_type=${documentType}`);
    return response.data;
  },

  /** Експорт документів */
  async exportDocuments(params: SearchParams & { format?: 'excel' | 'csv' }): Promise<Blob> {
    const response = await api.get<Blob>('/documents/export', {
      params,
      responseType: 'blob',
    });
    return response.data;
  },

  /** Друк документа */
  async printDocument(id: string, documentType: DocumentType): Promise<any> {
    const response = await api.get(`/documents/${id}/print?document_type=${documentType}`);
    return response.data;
  },
};
