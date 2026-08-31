import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useReceipt } from '@/hooks/useReceipts';
import { Button } from '@/components/ui/Button';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Printer, Receipt, ExternalLink, FileCheck2, Loader2, RefreshCw } from 'lucide-react';

import { useBackNavigation } from '@/hooks/useBackNavigation';
import { prroService } from '@/services/prroService';
import { getFiscalStatusLabel } from '@/types/receipt';
import PrintReceiptDialog from '@/components/pos/PrintReceiptDialog';
const ReceiptDetailPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const [showPrintDialog, setShowPrintDialog] = useState(false);
  const { id } = useParams<{ id: string }>();
  const { data: receipt, isLoading } = useReceipt(id || '');

  // ── Фіскальні реквізити (з v2) ─────────────────────────────────
  const [fiscalInfo, setFiscalInfo] = useState<{
    fiscal_status: string;
    fiscal_number: string | null;
    fiscal_sent_at: string | null;
    fiscal_error: string | null;
    fiscal_check_url: string | null;
  } | null>(null);
  const [fiscalLoading, setFiscalLoading] = useState(false);

  const loadFiscalInfo = async () => {
    if (!id) return;
    setFiscalLoading(true);
    try {
      const fiscal = await prroService.getReceiptFiscalInfo(id);
      setFiscalInfo({
        fiscal_status: fiscal.fiscal_status,
        fiscal_number: fiscal.fiscal_number,
        fiscal_sent_at: fiscal.fiscal_sent_at,
        fiscal_error: fiscal.fiscal_error,
        fiscal_check_url: fiscal.fiscal_check_url,
      });
    } catch {
      setFiscalInfo(null);
    } finally {
      setFiscalLoading(false);
    }
  };

  useEffect(() => {
    loadFiscalInfo();
  }, [id]);

  const formatDate = (dateStr: string) => {
    const d = new Date(dateStr);
    return d.toLocaleDateString('uk-UA', {
      day: '2-digit',
      month: 'long',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  const formatAmount = (amount: string) => {
    return parseFloat(amount).toLocaleString('uk-UA', {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  };

  const handleProductClick = (productId: string) => {
    // Відкриваємо картку товару в поточній вкладці
    // goBack() потім поверне назад в чек
    navigate(`/products/${productId}/edit`);
  };

  if (isLoading) {
    return (
      <div className="flex justify-center py-12">
        <Spinner size="lg" />
      </div>
    );
  }

  if (!receipt) {
    return (
      <div className="text-center py-12">
        <p className="text-gray-500 dark:text-gray-400">Чек не знайдено</p>
        <Button variant="secondary" className="mt-4" onClick={goBack}>
          Назад до списку
        </Button>
      </div>
    );
  }

  return (
    <div className="max-w-5xl mx-auto space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Button variant="ghost" onClick={goBack}>
            <ArrowLeft className="w-5 h-5" />
          </Button>
          <div>
            <h1 className="text-2xl font-bold text-gray-900 dark:text-white">
              Чек {receipt.receipt_number}
            </h1>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              {formatDate(receipt.created_at)}
            </p>
          </div>
        </div>
        <Button variant="secondary" onClick={() => setShowPrintDialog(true)}>
          <Printer className="w-4 h-4 mr-2" />
          Друк
        </Button>
      </div>

      {/* Receipt Card */}
      <div className="bg-white dark:bg-slate-800 rounded-lg shadow-sm border border-gray-200 dark:border-slate-700 overflow-hidden">
        {/* Receipt Header */}
        <div className="border-b border-gray-200 dark:border-slate-700 px-8 py-5 text-center">
          <Receipt className="w-8 h-8 mx-auto text-primary-600 mb-2" />
          <h2 className="text-xl font-bold text-gray-900 dark:text-white">КАСА</h2>
          <p className="text-sm text-gray-500">Фіскальний чек</p>
        </div>

        {/* Receipt Info */}
        <div className="px-8 py-5 border-b border-gray-200 dark:border-slate-700 space-y-1.5">
          <div className="flex justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Номер:</span>
            <span className="font-medium text-gray-900 dark:text-white">{receipt.receipt_number}</span>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Тип:</span>
            <span className={`font-medium ${
              receipt.receipt_type === 'sale'
                ? 'text-green-600 dark:text-green-400'
                : 'text-red-600 dark:text-red-400'
            }`}>
              {receipt.receipt_type === 'sale' ? 'ПРОДАЖ' : 'ПОВЕРНЕННЯ'}
            </span>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Касир:</span>
            <span className="font-medium text-gray-900 dark:text-white">
              {receipt.cashier_name || 'Невідомо'}
            </span>
          </div>
          <div className="flex justify-between text-sm">
            <span className="text-gray-500 dark:text-gray-400">Дата:</span>
            <span className="font-medium text-gray-900 dark:text-white">
              {formatDate(receipt.created_at)}
            </span>
          </div>
        </div>

        {/* Фіскальні реквізити (ПРРО) */}
        {(fiscalLoading || fiscalInfo || receipt?.fiscal_status) && (
          <div className="px-8 py-5 border-b border-gray-200 dark:border-slate-700">
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                Фіскалізація (ПРРО)
              </h3>
              <button
                onClick={loadFiscalInfo}
                className="text-xs text-gray-400 hover:text-primary-600 transition-colors inline-flex items-center gap-1"
                title="Оновити статус"
              >
                {fiscalLoading ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <RefreshCw className="w-3.5 h-3.5" />
                )}
              </button>
            </div>

            {fiscalLoading && !fiscalInfo ? (
              <div className="flex justify-center py-4">
                <Loader2 className="w-6 h-6 animate-spin text-primary-500" />
              </div>
            ) : (fiscalInfo || receipt) ? (
              <div className="space-y-2">
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-40">Статус:</span>
                  {(fiscalInfo?.fiscal_status || receipt?.fiscal_status) === 'sent' && (
                    <Badge variant="success">
                      {getFiscalStatusLabel(fiscalInfo?.fiscal_status || receipt?.fiscal_status)}
                    </Badge>
                  )}
                  {(fiscalInfo?.fiscal_status || receipt?.fiscal_status) === 'pending' && (
                    <Badge variant="warning">
                      {getFiscalStatusLabel(fiscalInfo?.fiscal_status || receipt?.fiscal_status)}
                    </Badge>
                  )}
                  {(fiscalInfo?.fiscal_status || receipt?.fiscal_status) === 'failed' && (
                    <Badge variant="danger">
                      {getFiscalStatusLabel(fiscalInfo?.fiscal_status || receipt?.fiscal_status)}
                    </Badge>
                  )}
                  {(!fiscalInfo?.fiscal_status || fiscalInfo.fiscal_status === 'none') &&
                    (!receipt?.fiscal_status || receipt.fiscal_status === 'none') && (
                    <Badge variant="default">
                      {getFiscalStatusLabel(fiscalInfo?.fiscal_status || receipt?.fiscal_status || 'none')}
                    </Badge>
                  )}
                </div>

                {(fiscalInfo?.fiscal_number || receipt?.fiscal_number) && (
                  <div className="flex items-center gap-3">
                    <FileCheck2 className="w-4 h-4 text-gray-400" />
                    <span className="text-sm text-gray-500 dark:text-gray-400 w-40">Фіскальний номер:</span>
                    <span className="text-sm font-medium text-gray-900 dark:text-white">
                      {fiscalInfo?.fiscal_number || receipt?.fiscal_number}
                    </span>
                  </div>
                )}

                {(fiscalInfo?.fiscal_sent_at || receipt?.fiscal_sent_at) && (
                  <div className="flex items-center gap-3">
                    <FileCheck2 className="w-4 h-4 text-gray-400" />
                    <span className="text-sm text-gray-500 dark:text-gray-400 w-40">Дата фіскалізації:</span>
                    <span className="text-sm font-medium text-gray-900 dark:text-white">
                      {formatDate(fiscalInfo?.fiscal_sent_at || receipt?.fiscal_sent_at || '')}
                    </span>
                  </div>
                )}

                {(fiscalInfo?.fiscal_error || receipt?.fiscal_error) && (
                  <div className="flex items-start gap-3">
                    <FileCheck2 className="w-4 h-4 text-danger-500 mt-0.5" />
                    <span className="text-sm text-gray-500 dark:text-gray-400 w-40">Помилка:</span>
                    <span className="text-sm text-danger-600 dark:text-danger-400">
                      {fiscalInfo?.fiscal_error || receipt?.fiscal_error}
                    </span>
                  </div>
                )}

                {(fiscalInfo?.fiscal_check_url || receipt?.fiscal_check_url) && (
                  <div className="mt-2">
                    <a
                      href={fiscalInfo?.fiscal_check_url || receipt?.fiscal_check_url || '#'}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-xs text-primary-600 hover:text-primary-700 dark:text-primary-400 underline"
                    >
                      Перевірити чек на сайті ДПС →
                    </a>
                  </div>
                )}
              </div>
            ) : (
              <p className="text-sm text-gray-400">Немає даних про фіскалізацію</p>
            )}
          </div>
        )}

        {/* Банківська транзакція (картковий термінал ПриватБанк) */}
        {receipt.terminal_rrn && (
          <div className="px-8 py-5 border-b border-gray-200 dark:border-slate-700">
            <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
              Банківська транзакція
            </h3>
            <div className="space-y-2">
              <div className="flex items-center gap-3">
                <FileCheck2 className="w-4 h-4 text-gray-400" />
                <span className="text-sm text-gray-500 dark:text-gray-400 w-44">RRN:</span>
                <span className="text-sm font-medium text-gray-900 dark:text-white font-mono">
                  {receipt.terminal_rrn}
                </span>
              </div>
              {receipt.terminal_approval_code && (
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-44">Код авторизації:</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white font-mono">
                    {receipt.terminal_approval_code}
                  </span>
                </div>
              )}
              {receipt.terminal_invoice_number && (
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-44">Номер чека терміналу:</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {receipt.terminal_invoice_number}
                  </span>
                </div>
              )}
              {receipt.terminal_card_pan && (
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-44">Картка:</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white font-mono">
                    {receipt.terminal_card_pan}
                  </span>
                </div>
              )}
              {receipt.terminal_payment_system && (
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-44">МПС:</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {receipt.terminal_payment_system}
                  </span>
                </div>
              )}
              {receipt.terminal_status && (
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-44">Статус:</span>
                  <span className="text-sm font-medium text-green-600 dark:text-green-400 uppercase">
                    {receipt.terminal_status}
                  </span>
                </div>
              )}
              {receipt.terminal_created_at && (
                <div className="flex items-center gap-3">
                  <FileCheck2 className="w-4 h-4 text-gray-400" />
                  <span className="text-sm text-gray-500 dark:text-gray-400 w-44">Дата транзакції:</span>
                  <span className="text-sm font-medium text-gray-900 dark:text-white">
                    {formatDate(receipt.terminal_created_at)}
                  </span>
                </div>
              )}
            </div>
          </div>
        )}

        {/* Items */}
        <div className="px-8 py-5 border-b border-gray-200 dark:border-slate-700">
          <h3 className="text-sm font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-3">
            Товари
          </h3>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-gray-200 dark:border-slate-700">
                <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Товар</th>
                <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Штрихкод</th>
                <th className="text-center py-2 text-gray-500 dark:text-gray-400 font-medium">К-сть</th>
                <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Ціна</th>
                <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">ПДВ</th>
                <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Сума</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100 dark:divide-slate-700">
              {(receipt.items || []).map((item: any) => (
                <tr
                  key={item.id}
                  className="group cursor-pointer hover:bg-primary-50 dark:hover:bg-primary-900/10 transition-colors"
                  onClick={() => handleProductClick(item.product_id)}
                  title="Відкрити картку товару"
                >
                  <td className="py-2 text-gray-900 dark:text-white font-medium">
                    <span className="flex items-center gap-1.5">
                      {item.product_name}
                      <ExternalLink className="w-3 h-3 text-gray-300 group-hover:text-primary-500 transition-colors opacity-0 group-hover:opacity-100" />
                    </span>
                  </td>
                  <td className="py-2 text-gray-500 dark:text-gray-400 text-xs">
                    {item.product_barcode || '—'}
                  </td>
                  <td className="py-2 text-center text-gray-900 dark:text-white">
                    {parseFloat(item.quantity).toFixed(3)}
                  </td>
                  <td className="py-2 text-right text-gray-900 dark:text-white">
                    {formatAmount(item.price)}
                  </td>
                  <td className="py-2 text-right text-gray-500 dark:text-gray-400">
                    {item.vat_rate || 0}%
                  </td>
                  <td className="py-2 text-right font-medium text-gray-900 dark:text-white">
                    {formatAmount(item.total)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {/* Total */}
        <div className="px-8 py-5">
          <div className="flex justify-between items-center">
            <span className="text-xl font-bold text-gray-900 dark:text-white">ВСЬОГО:</span>
            <span className="text-2xl font-bold text-primary-600">
              {formatAmount(receipt.total_amount)} грн
            </span>
          </div>
        </div>
      </div>
      <PrintReceiptDialog
        isOpen={showPrintDialog}
        onClose={() => setShowPrintDialog(false)}
        receipt={receipt}
      />
    </div>
  );
};

export default ReceiptDetailPage;
