import React, { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useReceipt } from '@/hooks/useReceipts';
import { Button } from '@/components/ui/Button';
import { Spinner } from '@/components/ui/Spinner';
import { ArrowLeft, Printer, Receipt, ExternalLink } from 'lucide-react';

import { useBackNavigation } from '@/hooks/useBackNavigation';
import PrintReceiptDialog from '@/components/pos/PrintReceiptDialog';
const ReceiptDetailPage: React.FC = () => {
  const navigate = useNavigate();
  const { goBack } = useBackNavigation();
  const [showPrintDialog, setShowPrintDialog] = useState(false);
  const { id } = useParams<{ id: string }>();
  const { data: receipt, isLoading } = useReceipt(id || '');

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
