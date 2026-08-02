import React, { useMemo, useState, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import toast from 'react-hot-toast';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { ArrowLeft, CheckCircle, XCircle, BookOpen, Banknote, RefreshCw, ExternalLink, ShoppingCart, Calendar, Edit, Printer, ArrowUp, ArrowDown } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import api from '@/services/api';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { formatCurrency, formatDateTime, formatDocumentStatus, formatDocumentType } from '@/utils/format';
import PrintFromInvoiceModal from '@/components/printing/PrintFromInvoiceModal';

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
  if (pathname.includes('/inventory/')) return 'inventory';
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
    case 'inventory': return `/inventory/${id}`;
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
    case 'inventory': return 'Інвентаризація';
    default: return 'Документ';
  }
}

/** Повертає правильний префікс API для типу документа */
function getDocumentTypeForApi(type: string): string {
  switch (type) {
    case 'invoice': return 'invoices';
    case 'purchase_order': return 'purchase-orders';
    case 'transfer': return 'transfers';
    case 'write_off': return 'write-offs';
    case 'return_invoice': return 'return-invoices';
    case 'inventory': return 'inventory';
    default: return 'documents';
  }
}

/** Обчислює націнку у відсотках на основі ціни продажу та собівартості */
function calcMarkupPercent(price: number, costPrice: number | null | undefined): number | null {
  if (!costPrice || costPrice <= 0 || !price || price <= 0) return null;
  return Math.round(((price - costPrice) / costPrice) * 100);
}

/** Компонент індикатора зміни ціни */
const PriceChangeIndicator: React.FC<{
  invoicePrice: number;
  currentPrice: number | undefined | null;
}> = ({ invoicePrice, currentPrice }) => {
  const oldPrice = currentPrice != null ? Number(currentPrice) : null;
  const newPrice = Number(invoicePrice);

  // Якщо старої ціни немає — показуємо ціну накладної без індикатора
  if (oldPrice === null) {
    return <span className="text-gray-900 dark:text-gray-100">{formatCurrency(newPrice)}</span>;
  }

  const isSame = Math.abs(oldPrice - newPrice) < 0.001;
  const isIncreased = newPrice > oldPrice;

  if (isSame) {
    return (
      <span className="inline-flex items-center gap-1.5">
        <span className="w-2.5 h-2.5 rounded-full bg-green-500 inline-block flex-shrink-0" />
        <span className="text-gray-900 dark:text-gray-100">{formatCurrency(newPrice)}</span>
      </span>
    );
  }

  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="w-2.5 h-2.5 rounded-full bg-red-500 inline-block flex-shrink-0" />
      <span className="flex flex-col items-end leading-tight">
        <span className="text-xs text-gray-400 dark:text-gray-500 line-through">
          {formatCurrency(oldPrice)}
        </span>
        <span className="text-sm font-bold text-red-600 dark:text-red-400 inline-flex items-center gap-0.5">
          {formatCurrency(newPrice)}
          {isIncreased
            ? <ArrowUp className="w-3 h-3 text-red-500" />
            : <ArrowDown className="w-3 h-3 text-red-500" />
          }
        </span>
      </span>
    </span>
  );
};

const DocumentViewPage: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const location = useLocation();

  const queryClient = useQueryClient();
  const docType = getDocumentTypeFromPath(location.pathname);
  const docTitle = getDocumentTitle(docType);

  // Стан для модалки друку
  const [showPrintModal, setShowPrintModal] = useState(false);

  const { data: doc, isLoading, error } = useQuery({
    queryKey: ['document', docType, id],
    queryFn: async () => {
      const endpoint = getApiEndpoint(docType, id!);
      const response = await api.get(endpoint);
      return response.data;
    },
    enabled: !!id,
  });

  // Підрахунок товарів зі змінною ціною (для інвойсів)
  const { changedPriceCount, totalItems } = useMemo(() => {
    if (!doc?.items || !Array.isArray(doc.items)) {
      return { changedPriceCount: 0, totalItems: 0 };
    }
    const changed = doc.items.filter((item: any) => {
      const invoicePrice = Number(item.price || 0);
      // Показуємо зміну: previous_price (ціна до накладної) або поточна ціна товару
      const prevPrice = item.previous_price != null ? Number(item.previous_price) : (item.product?.price != null ? Number(item.product.price) : null);
      if (prevPrice === null) return false;
      return Math.abs(prevPrice - invoicePrice) >= 0.001;
    });
    return {
      changedPriceCount: changed.length,
      totalItems: doc.items.length,
    };
  }, [doc]);

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
    <div className="max-w-7xl mx-auto space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <button aria-label="Назад"
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
            <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
              Оновлено {formatDateTime(doc.updated_at)}
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
                    {doc.supplier_name || '-'}
                  </p>
                )}
              </div>

              {/* Спосіб оплати */}
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Спосіб оплати</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.payment_method === 'credit' ? 'В борг постачальнику' :
                   doc.payment_method === 'bank_transfer' ? 'По перерахунку' :
                   doc.payment_method === 'cash' ? 'Готівкою з каси' :
                   doc.payment_method === 'other' ? 'Інший спосіб' :
                   doc.payment_method || 'Не вказано'}
                </p>
              </div>

              {/* Фіскальний */}
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Тип</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.is_fiscal ? 'Фіскальна' : 'Нефіскальна'}
                </p>
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
                    {doc.supplier_name || '-'}
                  </p>
                )}
              </div>

              {/* Фіскальний */}
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Тип</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.is_fiscal ? 'Фіскальний' : 'Нефіскальний'}
                </p>
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
                    {doc.supplier_name || '-'}
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

              {/* Фіскальний */}
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Тип</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.is_fiscal ? 'Фіскальний' : 'Нефіскальний'}
                </p>
              </div>
            </>
          )}

          {/* Для inventory показуємо дату, локацію та підсумки */}
          {docType === 'inventory' && (
            <>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Дата інвентаризації</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.inventory_date ? new Date(doc.inventory_date).toLocaleDateString('uk-UA') : '-'}
                </p>
              </div>
              <div>
                <p className="text-sm text-gray-500 dark:text-gray-400">Місце проведення</p>
                <p className="font-medium text-gray-900 dark:text-gray-100">
                  {doc.location || '-'}
                </p>
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
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                {docType === 'return_invoice' ? 'Повернуті товари' : 'Товари'}
              </h3>
              {/* Кнопка друку цінників/етикеток — для invoice */}
              {docType === 'invoice' && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => setShowPrintModal(true)}
                  icon={<Printer className="w-4 h-4" />}
                >
                  Друк цінників / етикеток
                </Button>
              )}
            </div>
            <div className="border border-gray-200 dark:border-slate-700 rounded-xl overflow-hidden">
              <table className="w-full">
                <thead>
                  <tr className="bg-gray-50 dark:bg-slate-800/50">
                    <th className="table-header">Товар</th>
                    {docType === 'inventory' ? (
                      <>
                        <th className="table-header w-24 text-right">Облікова к-сть</th>
                        <th className="table-header w-24 text-right">Фактична к-сть</th>
                        <th className="table-header w-24 text-right">Різниця</th>
                        <th className="table-header w-28 text-right">Собівартість (з ПДВ)</th>
                        <th className="table-header w-28 text-right">Ціна продажу</th>
                        <th className="table-header w-28 text-right">Націнка</th>
                        <th className="table-header w-28 text-right">Сума собівартості</th>
                        <th className="table-header w-28 text-right">Сума продажу</th>
                        <th className="table-header w-28 text-right">Сума відхилення</th>
                      </>
                    ) : (
                      <>
                        <th className="table-header w-24 text-right">Кількість</th>
                        {docType === 'invoice' && (
                          <th className="table-header w-32 text-right">Ціна</th>
                        )}
                        <th className="table-header w-28 text-right">Собівартість (з ПДВ)</th>
                        <th className="table-header w-28 text-right">Ціна продажу</th>
                        <th className="table-header w-24 text-right">Націнка</th>
                        <th className="table-header w-28 text-right">Сума собівартості</th>
                        <th className="table-header w-28 text-right">Сума продажу</th>
                        {docType === 'return_invoice' && (
                          <th className="table-header w-28 text-right">Сума відхилення</th>
                        )}
                      </>
                    )}
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {doc.items.map((item: any) => {
                    const productId = item.product?.id || item.product_id;
                    const costPrice = Number(item.cost_price || 0);
                    const sellPrice = Number(item.price || 0);
                    // previous_price — ціна товару ДО створення накладної (для показу змін навіть після підтвердження)
                    const currentPrice = item.previous_price != null ? Number(item.previous_price) : (item.product?.price != null ? Number(item.product.price) : null);
                    const quantity = Number(item.quantity || 0);
                    const markup = calcMarkupPercent(sellPrice, costPrice);
                    // Для return_invoice: сума відхилення = sellPrice*quantity - costPrice*quantity
                    const deviation = docType === 'return_invoice'
                      ? sellPrice * quantity - costPrice * quantity
                      : 0;
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

                        {docType === 'inventory' ? (
                          <>
                            <td className="table-cell text-right">{Number(item.accounting_quantity || 0).toFixed(3)}</td>
                            <td className="table-cell text-right">{Number(item.actual_quantity || 0).toFixed(3)}</td>
                            <td className="table-cell text-right">
                              <span className={`font-medium ${
                                Number(item.difference || 0) > 0
                                  ? 'text-green-600 dark:text-green-400'
                                  : Number(item.difference || 0) < 0
                                  ? 'text-red-600 dark:text-red-400'
                                  : ''
                              }`}>
                                {Number(item.difference || 0) > 0 ? '+' : ''}{Number(item.difference || 0).toFixed(3)}
                              </span>
                            </td>
                            <td className="table-cell text-right">{formatCurrency(costPrice)}</td>
                            <td className="table-cell text-right">{formatCurrency(sellPrice)}</td>
                            <td className="table-cell text-right">
                              {markup !== null ? (
                                <span className="text-green-600 dark:text-green-400">{markup}%</span>
                              ) : (
                                <span className="text-gray-400">-</span>
                              )}
                            </td>
                            <td className="table-cell text-right font-medium">{formatCurrency(Number(item.actual_quantity || 0) * costPrice)}</td>
                            <td className="table-cell text-right font-medium">{formatCurrency(Number(item.actual_quantity || 0) * sellPrice)}</td>
                            <td className="table-cell text-right">
                              <span className={`font-medium ${
                                Number(item.difference || 0) * costPrice > 0
                                  ? 'text-green-600 dark:text-green-400'
                                  : Number(item.difference || 0) * costPrice < 0
                                  ? 'text-red-600 dark:text-red-400'
                                  : ''
                              }`}>
                                {formatCurrency(Number(item.difference || 0) * costPrice)}
                              </span>
                            </td>
                          </>
                        ) : (
                          <>
                            <td className="table-cell text-right">{quantity.toFixed(3)}</td>
                            {/* Колонка "Ціна" з індикатором — тільки для invoice */}
                            {docType === 'invoice' && (
                              <td className="table-cell text-right">
                                <PriceChangeIndicator
                                  invoicePrice={sellPrice}
                                  currentPrice={currentPrice}
                                />
                              </td>
                            )}
                            <td className="table-cell text-right">{formatCurrency(costPrice || sellPrice)}</td>
                            <td className="table-cell text-right">{formatCurrency(sellPrice)}</td>
                            <td className="table-cell text-right">
                              {markup !== null ? (
                                <span className="text-green-600 dark:text-green-400">{markup}%</span>
                              ) : (
                                <span className="text-gray-400">-</span>
                              )}
                            </td>
                            <td className="table-cell text-right font-medium">{formatCurrency((costPrice || sellPrice) * quantity)}</td>
                            <td className="table-cell text-right font-medium">{formatCurrency(Number(item.total))}</td>
                            {docType === 'return_invoice' && (
                              <td className="table-cell text-right">
                                <span className={`font-medium ${
                                  deviation > 0
                                    ? 'text-green-600 dark:text-green-400'
                                    : deviation < 0
                                    ? 'text-red-600 dark:text-red-400'
                                    : ''
                                }`}>
                                  {deviation > 0 ? '+' : ''}{formatCurrency(deviation)}
                                </span>
                              </td>
                            )}
                          </>
                        )}
                      </tr>
                    );
                  })}
                </tbody>
                {docType !== 'inventory' && (
                  <tfoot>
                    <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                      <td colSpan={docType === 'return_invoice' ? 5 : docType === 'invoice' ? 5 : 4} className="px-4 py-3 text-right text-gray-700 dark:text-gray-300">
                        Закупівельна сума:
                      </td>
                      <td colSpan={docType === 'return_invoice' ? 3 : docType === 'invoice' ? 3 : 2} className="px-4 py-3 font-bold text-xl text-gray-900 dark:text-gray-100 text-right">
                        {(() => {
                          const total = (doc.items || []).reduce((sum: number, item: any) =>
                            sum + Number(item.cost_price || item.price || 0) * Number(item.quantity || 0), 0
                          );
                          return formatCurrency(total);
                        })()}
                      </td>
                    </tr>
                    <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                      <td colSpan={docType === 'return_invoice' ? 5 : docType === 'invoice' ? 5 : 4} className="px-4 py-3 text-right text-gray-700 dark:text-gray-300">
                        Сума продажу:
                      </td>
                      <td colSpan={docType === 'return_invoice' ? 3 : docType === 'invoice' ? 3 : 2} className="px-4 py-3 font-bold text-gray-900 dark:text-gray-100 text-right">
                        {formatCurrency(Number(doc.total_amount))}
                      </td>
                    </tr>
                  </tfoot>
                )}
                {docType === 'inventory' && (
                  <tfoot>
                    <tr className="bg-gray-50 dark:bg-slate-800/50 font-semibold">
                      <td className="px-4 py-3 text-gray-700 dark:text-gray-300">Загалом:</td>
                      <td className="px-4 py-3 text-right text-gray-900 dark:text-gray-100">
                        {(doc.items || []).reduce((s: number, i: any) => s + Number(i.accounting_quantity || 0), 0).toFixed(3)}
                      </td>
                      <td className="px-4 py-3 text-right text-gray-900 dark:text-gray-100">
                        {(doc.items || []).reduce((s: number, i: any) => s + Number(i.actual_quantity || 0), 0).toFixed(3)}
                      </td>
                      <td className="px-4 py-3 text-right">
                        {(() => {
                          const totalDiff = (doc.items || []).reduce((s: number, i: any) => s + Number(i.difference || 0), 0);
                          return (
                            <span className={`font-bold text-lg ${
                              totalDiff > 0 ? 'text-green-600 dark:text-green-400' :
                              totalDiff < 0 ? 'text-red-600 dark:text-red-400' : ''
                            }`}>
                              {totalDiff > 0 ? '+' : ''}{totalDiff.toFixed(3)}
                            </span>
                          );
                        })()}
                      </td>
                      <td></td>
                      <td></td>
                      <td></td>
                      <td className="px-4 py-3 text-right font-bold text-blue-700 dark:text-blue-400 text-lg">
                        {formatCurrency((doc.items || []).reduce((s: number, i: any) => s + Number(i.actual_quantity || 0) * Number(i.cost_price || 0), 0))}
                      </td>
                      <td className="px-4 py-3 text-right font-bold text-emerald-700 dark:text-emerald-400 text-lg">
                        {formatCurrency((doc.items || []).reduce((s: number, i: any) => s + Number(i.actual_quantity || 0) * Number(i.price || 0), 0))}
                      </td>
                      <td className="px-4 py-3 text-right font-bold text-lg"
                        style={{
                          color: (doc.items || []).reduce((s: number, i: any) => s + Number(i.difference || 0) * Number(i.cost_price || 0), 0) > 0
                            ? '#16a34a'
                            : (doc.items || []).reduce((s: number, i: any) => s + Number(i.difference || 0) * Number(i.cost_price || 0), 0) < 0
                            ? '#dc2626'
                            : 'inherit'
                        }}>
                        {formatCurrency((doc.items || []).reduce((s: number, i: any) => s + Number(i.difference || 0) * Number(i.cost_price || 0), 0))}
                      </td>
                    </tr>
                  </tfoot>
                )}
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
                    <th className="table-header w-28 text-right">Собівартість (з ПДВ)</th>
                    <th className="table-header w-28 text-right">Ціна продажу</th>
                    <th className="table-header w-24 text-right">Націнка</th>
                    <th className="table-header w-28 text-right">Сума собівартості</th>
                    <th className="table-header w-28 text-right">Сума продажу</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 dark:divide-slate-700">
                  {doc.exchange_invoice.items?.map((item: any) => {
                    const productId = item.product?.id || item.product_id;
                    const costPrice = Number(item.cost_price || 0);
                    const sellPrice = Number(item.price || 0);
                    const quantity = Number(item.quantity || 0);
                    const markup = calcMarkupPercent(sellPrice, costPrice);
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
                        <td className="table-cell text-right">{quantity.toFixed(3)}</td>
                        <td className="table-cell text-right">{formatCurrency(costPrice || sellPrice)}</td>
                        <td className="table-cell text-right">{formatCurrency(sellPrice)}</td>
                        <td className="table-cell text-right">
                          {markup !== null ? (
                            <span className="text-green-600 dark:text-green-400">{markup}%</span>
                          ) : (
                            <span className="text-gray-400">-</span>
                          )}
                        </td>
                        <td className="table-cell text-right font-medium">{formatCurrency((costPrice || sellPrice) * quantity)}</td>
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
          {doc.status === 'draft' && (
            <Button 
              variant="primary"
              onClick={async () => {
                try {
                  await api.post(`/${getDocumentTypeForApi(docType)}/${id}/confirm`, {
                    status: 'confirmed'
                  });
                  toast.success('Накладну підтверджено');
                  // Перезавантажуємо дані
                  queryClient.invalidateQueries({ queryKey: ['document', docType, id] });
                } catch (e: any) {
                  toast.error(e?.response?.data?.detail || 'Помилка підтвердження');
                }
              }}
              className="flex items-center gap-2"
            >
              <CheckCircle className="w-4 h-4" />
              Провести
            </Button>
          )}
          <Button 
            variant="secondary"
            onClick={() => navigate(docType === "return_invoice" ? `/documents/return/${id}/edit` : `/documents/${docType}/${id}/edit`)}
            className="flex items-center gap-2"
          >
            <Edit className="w-4 h-4" />
            Редагувати
          </Button>
          <Button variant="secondary" onClick={goBack}>
            До списку
          </Button>
        </div>
      </div>

      {/* Модалка друку цінників/етикеток з накладної */}
      <PrintFromInvoiceModal
        isOpen={showPrintModal}
        onClose={() => setShowPrintModal(false)}
        invoiceId={id!}
        totalItems={totalItems}
        changedPriceCount={changedPriceCount}
      />
    </div>
  );
};

export default DocumentViewPage;
