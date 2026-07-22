import React from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { ArrowLeft, CheckCircle, XCircle, BookOpen, Banknote, RefreshCw, ExternalLink, ShoppingCart, Calendar } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import api from '@/services/api';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { formatCurrency, formatDateTime, formatDocumentStatus, formatDocumentType } from '@/utils/format';

import { useBackNavigation } from '@/hooks/useBackNavigation';
const statusBadgeVariant: Record<string, 'default' | 'success' | 'danger' | 'warning'> = {
  draft: 'warning',
  confirmed: 'success',
  cancelled: 'danger',
};

/** Мапа типів дій повернення на мітки */
const RETURN_ACTION_LABELS: Record<string, { label: string; icon: React.ReactNode; color: string }> = {
  deduct_from_debt: {
    label: 'Списано з боргу',
    icon: <BookOpen className="w-3.5 h-3.5" />,
    color: 'text-blue-600 bg-blue-50 dark:bg-blue-900/20',
  },
  add_to_cash: {
    label: 'Зачислено в касу',
    icon: <Banknote className="w-3.5 h-3.5" />,
    color: 'text-green-600 bg-green-50 dark:bg-green-900/20',
  },
  exchange: {
    label: 'Обмін на інший товар',
    icon: <RefreshCw className="w-3.5 h-3.5" />,
    color: 'text-purple-600 bg-purple-50 dark:bg-purple-900/20',
  },
};

/** Визначає тип документа з URL шляху */
function getDocumentTypeFromPath(pathname: string): string {
  if (pathname.includes('/invoice/')) return 'invoice';
  if (pathname.includes('/purchase-order/')) return 'purchase_order';
  if (pathname.includes('/transfer/')) return 'transfer';
  if (pathname.includes('/write-off/')) return 'write_off';
  if (pathname.includes('/return/')) return 'return_invoice';
  return 'invoice';
}

/** Повертає правильний ендпоінт API для типу документа */
function getApiEndpoint(type: string, id: string): string {
  switch (type) {
    case 'invoice': return `/invoices/${id}`;
    case 'purchase_order': return `/purchase-orders/${id}`;
    case 'transfer': return `/transfers/${id}`;
    case 'write_off': return `/write-offs/${id}`;
    case 'return_invoice': return `/return-invoices/${id}`;
    default: return `/documents/${id}`;
  }
}

/** Людська назва типу документа */
function getDocumentTitle(type: string): string {
  switch (type) {
    case 'invoice': return 'Прибуткова накладна';
    case 'purchase_order': return 'Замовлення постачальнику';
    case 'transfer': return 'Переміщення';
    case 'write_off': return 'Списання';
    case 'return_invoice': return 'Повернення';
    default: return 'Документ';
  }
}

const DocumentViewPage: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const location = useLocation();

  const docType = getDocumentTypeFromPath(location.pathname);
  const docTitle = getDocumentTitle(docType);

  const { data: doc, isLoading, error } = useQuery({
    queryKey: ['document', docType, id],
    queryFn: async () => {
      const endpoint = getApiEndpoint(docType, id!);
      const response = await api.get(endpoint);
      return response.data;
    },
    enabled: !!id,
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Spinner size="lg" />
      </div>
    );
  }

  if (error || !doc) {
    return (
      <div className="text-center py-12">
        <p className="text-danger-600 font-medium">Помилка завантаження документа</p>
        <p className="text-sm text-gray-500 mt-2">
          {docType === 'write_off'
            ? 'Не вдалося завантажити списання. Можливо, документ не існує або стався збій на сервері.'
            : 'Перевірте правильність ID документа та спробуйте ще раз.'}
        </p>
        <Button variant="secondary" onClick={goBack} className="mt-4">
          Повернутись до списку
        </Button>
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button
            onClick={goBack}
            className="p-2 rounded-lg text-gray-400 hover:text-gray-600 hover:bg-gray-100 dark:hover:bg-slate-700 transition-colors"
          >
            <ArrowLeft className="w-5 h-5" />
          </button>
          <div>
            <h2 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
              {docTitle} №{doc.number || doc.document_number || '-'}
            </h2>
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Створено {formatDateTime(doc.created_at)}
            </p>
          </div>
        </div>
        <Badge variant={statusBadgeVariant[doc.status] || 'default'} className="text-sm px-3 py-1">
          {formatDocumentStatus(doc.status)}
        </Badge>
      </div>

      {/* Card with details */}
      <div className="card p-6 space-y-6">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div>
            <p className="text-sm text-gray-500 dark:text-gray-400">Номер</p>
            <p className="font-medium text-gray-900 dark:text-gray-100">{doc.number || doc.document_number || '-'}</p>
          </div>

          {/* Для invoice показуємо дату накладної та постачальника */}
          {docType === 'invoice' && (
            <>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Дата накладної</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.invoice_date ? new Date(doc.invoice_date).toLocaleDateString('uk-UA') : '-'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Постачальник</p>
                {doc.supplier?.id ? (
                  <button
                    onClick={() => navigate(`/suppliers/${doc.supplier.id}/edit`)}
                    className="font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300 hover:underline text-left"
                  >
                    {doc.supplier.name}
                  </button>
                ) : (
                  <p className="font-medium text-gray-900 dark:text-gray-100">
                    {doc.supplier?.name || doc.supplier_name || '-'}
                  </p>
                )}
              </div>
            </>
          )}

          {/* Для purchase_order показуємо дати та постачальника */}
          {docType === 'purchase_order' && (
            <>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Дата замовлення</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.order_date ? new Date(doc.order_date).toLocaleDateString('uk-UA') : '-'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Очікувана поставка</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.expected_date ? new Date(doc.expected_date).toLocaleDateString('uk-UA') : 'Не вказано'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Постачальник</p>
                {doc.supplier?.id ? (
                  <button
                    onClick={() => navigate(`/suppliers/${doc.supplier.id}/edit`)}
                    className="font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300 hover:underline text-left"
                  >
                    {doc.supplier.name}
                  </button>
                ) : (
                  <p className="font-medium text-gray-900 dark:text-gray-100">
                    {doc.supplier?.name || doc.supplier_name || '-'}
                  </p>
                )}
              </div>
            </>
          )}

          {/* Для write_off показуємо причину та дату списання */}
          {docType === 'write_off' && (
            <>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Причина списання</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.reason === 'expired' ? 'Прострочений термін' :
                   doc.reason === 'damaged' ? 'Пошкодження' :
                   doc.reason === 'lost' ? 'Втрата' :
                   doc.reason === 'other' ? 'Інше' : doc.reason || '-'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Дата списання</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.write_off_date ? new Date(doc.write_off_date).toLocaleDateString('uk-UA') : '-'}
                </p>
              </div>
            </>
          )}

          {/* Для transfer показуємо склади */}
          {docType === 'transfer' && (
            <>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Звідки</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.from_warehouse?.name || doc.from_warehouse_name || '-'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Куди</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.to_warehouse?.name || doc.to_warehouse_name || '-'}
                </p>
              </div>
            </>
          )}

          {/* Для return_invoice показуємо постачальника, дату та дію */}
          {docType === 'return_invoice' && (
            <>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Дата повернення</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.return_date ? new Date(doc.return_date).toLocaleDateString('uk-UA') : '-'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Постачальник</p>
                {doc.supplier?.id ? (
                  <button
                    onClick={() => navigate(`/suppliers/${doc.supplier.id}/edit`)}
                    className="font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300 hover:underline text-left"
                  >
                    {doc.supplier.name}
                  </button>
                ) : (
                  <p className="font-medium text-gray-900 dark:text-gray-100">
                    {doc.supplier?.name || doc.supplier_name || '-'}
                  </p>
                )}
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Дія</p>
                {doc.return_action && RETURN_ACTION_LABELS[doc.return_action] ? (
                  <div className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium mt-0.5 ${RETURN_ACTION_LABELS[doc.return_action].color}`}>
                    {RETURN_ACTION_LABELS[doc.return_action].icon}
                    {RETURN_ACTION_LABELS[doc.return_action].label}
                  </div>
                ) : (
                  <p className="font-medium text-gray-900 dark:text-gray-100">-</p>
                )}
              </div>
            </>
          )}
        </div>

        {doc.notes && (
          <div>
            <p className="text-sm text-gray-500 dark:text-gray-400">Примітки</p>
            <p className="text-gray-900 dark:text-gray-100">{doc.notes}</p>
          </div>
        )}

        {/* ─── Товари ─────────────────────────────────── */}
        {doc.items && doc.items.length > 0 && (
          <div>
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3">
              {docType === 'return_invoice' ? 'Повернуті товари' : 'Товари'}
            </h3>
            <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
              <table className="w-full">
                <thead>
                  <tr className="bg-gray-50 dark:bg-slate-800/50">
                    <th className="table-header">Товар</th>
                    <th className="table-header w-24 text-right">Кількість</th>
                    <th className="table-header w-28 text-right">Ціна</th>
                    <th className="table-header w-28 text-right">Сума</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {doc.items.map((item: any) => {
                    const productId = item.product?.id || item.product_id;
                    return (
                      <tr
                        key={item.id}
                        className={`group cursor-pointer transition-colors ${
                          productId
                            ? 'hover:bg-primary-50 dark:hover:bg-primary-900/10'
                            : ''
                        }`}
                        onClick={() => {
                          if (productId) navigate(`/products/${productId}/edit`);
                        }}
                        title={productId ? 'Відкрити картку товару' : undefined}
                      >
                        <td className="table-cell">
                          <p className="font-medium text-gray-900 dark:text-gray-100 flex items-center gap-1.5">
                            {item.product?.title || item.product_name || '-'}
                            {productId && (
                              <ExternalLink className="w-3 h-3 text-gray-300 group-hover:text-primary-500 transition-colors opacity-0 group-hover:opacity-100" />
                            )}
                          </p>
                          {item.product?.barcode && (
                            <p className="text-xs text-gray-400">ШК: {item.product.barcode}</p>
                          )}
                        </td>
                        <td className="table-cell text-right">{Number(item.quantity).toFixed(3)}</td>
                        <td className="table-cell text-right">{formatCurrency(Number(item.price))}</td>
                        <td className="table-cell text-right font-medium">{formatCurrency(Number(item.total))}</td>
                      </tr>
                    );
                  })}
                </tbody>
                <tfoot>
                  <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                    <td colSpan={3} className="px-4 py-3 text-right text-gray-700 dark:text-gray-300">
                      Загальна сума:
                    </td>
                    <td className="px-4 py-3 font-bold text-lg text-gray-900 dark:text-gray-100 text-right">
                      {formatCurrency(Number(doc.total_amount))}
                    </td>
                  </tr>
                </tfoot>
              </table>
            </div>
          </div>
        )}

        {/* ─── Прибуткова накладна при обміні (return_invoice) ── */}
        {docType === 'return_invoice' && doc.exchange_invoice && (
          <div className="border-t border-gray-200 dark:border-slate-700 pt-6">
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                <RefreshCw className="w-5 h-5 text-purple-500" />
                Обмін — прибуткова накладна
              </h3>
              <button
                onClick={() => navigate(`/documents/invoice/${doc.exchange_invoice.id}`)}
                className="inline-flex items-center gap-1.5 text-sm text-primary-600 hover:text-primary-700 dark:text-primary-400 font-medium"
              >
                <ExternalLink className="w-3.5 h-3.5" />
                Відкрити накладну
              </button>
            </div>
            <div
              className="bg-purple-50 dark:bg-purple-900/10 border border-purple-200 dark:border-purple-800 rounded-xl p-4 mb-3 cursor-pointer hover:bg-purple-100 dark:hover:bg-purple-900/20 transition-colors"
              onClick={() => navigate(`/documents/invoice/${doc.exchange_invoice.id}`)}
              title="Відкрити накладну"
            >
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Номер накладної</p>
                  <p className="font-medium text-primary-600 dark:text-primary-400 hover:underline">
                    {doc.exchange_invoice.number}
                  </p>
                </div>
                <div>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Сума</p>
                  <p className="font-medium text-gray-900 dark:text-gray-100">
                    {formatCurrency(Number(doc.exchange_invoice.total_amount))}
                  </p>
                </div>
              </div>
            </div>
            <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
              <table className="w-full">
                <thead>
                  <tr className="bg-gray-50 dark:bg-slate-800/50">
                    <th className="table-header">Новий товар</th>
                    <th className="table-header w-24 text-right">Кількість</th>
                    <th className="table-header w-28 text-right">Ціна</th>
                    <th className="table-header w-28 text-right">Сума</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {doc.exchange_invoice.items?.map((item: any) => {
                    const productId = item.product?.id || item.product_id;
                    return (
                      <tr
                        key={item.id}
                        className={`group cursor-pointer transition-colors ${
                          productId
                            ? 'hover:bg-primary-50 dark:hover:bg-primary-900/10'
                            : ''
                        }`}
                        onClick={() => {
                          if (productId) navigate(`/products/${productId}/edit`);
                        }}
                        title={productId ? 'Відкрити картку товару' : undefined}
                      >
                        <td className="table-cell">
                          <p className="font-medium text-gray-900 dark:text-gray-100 flex items-center gap-1.5">
                            {item.product?.title || item.product_name || '-'}
                            {productId && (
                              <ExternalLink className="w-3 h-3 text-gray-300 group-hover:text-primary-500 transition-colors opacity-0 group-hover:opacity-100" />
                            )}
                          </p>
                          {item.product?.barcode && (
                            <p className="text-xs text-gray-400">ШК: {item.product.barcode}</p>
                          )}
                        </td>
                        <td className="table-cell text-right">{Number(item.quantity).toFixed(3)}</td>
                        <td className="table-cell text-right">{formatCurrency(Number(item.price))}</td>
                        <td className="table-cell text-right font-medium">{formatCurrency(Number(item.total))}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {/* ─── Прибуткова накладна із замовлення (purchase_order) ── */}
        {docType === 'purchase_order' && doc.invoice && (
          <div className="border-t border-gray-200 dark:border-slate-700 pt-6">
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                <ShoppingCart className="w-5 h-5 text-green-500" />
                Створена прибуткова накладна
              </h3>
              <button
                onClick={() => navigate(`/documents/invoice/${doc.invoice.id}`)}
                className="inline-flex items-center gap-1.5 text-sm text-primary-600 hover:text-primary-700 dark:text-primary-400 font-medium"
              >
                <ExternalLink className="w-3.5 h-3.5" />
                Відкрити накладну
              </button>
            </div>
            <div
              className="bg-green-50 dark:bg-green-900/10 border border-green-200 dark:border-green-800 rounded-xl p-4 cursor-pointer hover:bg-green-100 dark:hover:bg-green-900/20 transition-colors"
              onClick={() => navigate(`/documents/invoice/${doc.invoice.id}`)}
              title="Відкрити накладну"
            >
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Номер накладної</p>
                  <p className="font-medium text-primary-600 dark:text-primary-400 hover:underline">
                    {doc.invoice.number}
                  </p>
                </div>
                <div>
                  <p className="text-xs text-gray-500 dark:text-gray-400">Сума</p>
                  <p className="font-medium text-gray-900 dark:text-gray-100">
                    {formatCurrency(Number(doc.invoice.total_amount))}
                  </p>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Якщо немає товарів */}
        {(!doc.items || doc.items.length === 0) && (
          <div className="text-center py-8 text-gray-400">
            <p>Немає товарів у цьому документі</p>
          </div>
        )}

        {/* Actions */}
        <div className="flex justify-end gap-3 pt-4 border-t border-gray-200 dark:border-slate-700">
          <Button variant="secondary" onClick={goBack}>
            До списку
          </Button>
        </div>
      </div>
    </div>
  );
};

export default DocumentViewPage;
