import React from 'react';
import { Link } from 'react-router-dom';
import { Phone, DollarSign, Receipt, CalendarDays } from 'lucide-react';
import { Debtor, DebtorPayment } from '@/services/debtorService';
import { Receipt as ReceiptType } from '@/types/receipt';
import { formatCurrency } from '@/utils/format';

interface DebtorCardProps {
  debtor: Debtor;
  receipts: ReceiptType[];
  payments: DebtorPayment[];
  onPay: (debtor: Debtor) => void;
}

interface CombinedEntry {
  id: string;
  type: 'receipt' | 'payment';
  date: string;
  receiptNumber?: string;
  totalAmount?: number;
  paidAmount?: number;
  debtAmount?: number;
  paymentAmount?: number;
  paymentMethod?: string | null;
  receiptId?: string;
}

const getCombinedEntries = (receipts: ReceiptType[], payments: DebtorPayment[]): CombinedEntry[] => {
  const entries: CombinedEntry[] = [];

  for (const r of receipts) {
    const paid = r.paid_amount ? parseFloat(r.paid_amount) : 0;
    const total = parseFloat(r.total_amount);
    entries.push({
      id: r.id,
      type: 'receipt',
      date: r.created_at,
      receiptNumber: r.receipt_number,
      totalAmount: total,
      paidAmount: paid >= total ? total : paid,
      debtAmount: Math.max(0, total - paid),
      receiptId: r.id,
    });
  }

  for (const p of payments) {
    entries.push({
      id: p.id,
      type: 'payment',
      date: p.created_at,
      paymentAmount: p.amount,
      paymentMethod: p.payment_method,
    });
  }

  // Сортуємо за датою (найновіші зверху)
  entries.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime());

  return entries;
};

const DebtorCard: React.FC<DebtorCardProps> = ({ debtor, receipts, payments, onPay }) => {
  const lastReceipt = receipts.length > 0 ? receipts[receipts.length - 1] : null;

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleDateString('uk-UA');
  };

  const combinedEntries = getCombinedEntries(receipts, payments);

  return (
    <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-gray-200 dark:border-slate-700 p-6 space-y-4">
      {/* Debtor Info */}
      <div className="flex items-start justify-between">
        <div className="space-y-1">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
            {debtor.name}
          </h3>
          {debtor.phone && (
            <div className="flex items-center gap-1.5 text-sm text-gray-500 dark:text-gray-400">
              <Phone className="w-4 h-4" />
              <span>{debtor.phone}</span>
            </div>
          )}
        </div>
        <div className="text-right">
          <p className="text-2xl font-bold text-danger-600">
            {formatCurrency(debtor.total_debt)}
          </p>
          {lastReceipt && (
            <div className="flex items-center gap-1 mt-1 text-xs text-gray-400 dark:text-gray-500 justify-end">
              <CalendarDays className="w-3.5 h-3.5" />
              <span>{formatDate(lastReceipt.created_at)}</span>
            </div>
          )}
        </div>
      </div>

      {/* Pay Button */}
      <button
        onClick={() => onPay(debtor)}
        className="w-full inline-flex items-center justify-center gap-2 px-4 py-2 bg-green-600 hover:bg-green-700 text-white text-sm font-medium rounded-lg transition-colors"
      >
        <DollarSign className="w-4 h-4" />
        Сплатити
      </button>

      {/* History Section */}
      <div>
        <div className="flex items-center gap-2 mb-3">
          <Receipt className="w-4 h-4 text-gray-400" />
          <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300">
            Історія
          </h4>
        </div>

        {combinedEntries.length === 0 ? (
          <p className="text-sm text-gray-400 dark:text-gray-500 text-center py-4">
            Немає записів
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-gray-200 dark:border-slate-700">
                  <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Тип</th>
                  <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Номер/Опис</th>
                  <th className="text-left py-2 text-gray-500 dark:text-gray-400 font-medium">Дата</th>
                  <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Сума</th>
                  <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Сплачено</th>
                  <th className="text-right py-2 text-gray-500 dark:text-gray-400 font-medium">Борг</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-slate-700">
                {combinedEntries.map((entry) => (
                  <tr key={entry.id} className={entry.type === 'payment' ? 'bg-green-50 dark:bg-green-900/10' : ''}>
                    <td className="py-2">
                      {entry.type === 'receipt' ? (
                        <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-300">
                          Чек
                        </span>
                      ) : (
                        <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-300">
                          Оплата
                        </span>
                      )}
                    </td>
                    <td className="py-2">
                      {entry.type === 'receipt' && entry.receiptId ? (
                        <Link
                          to={`/receipts/${entry.receiptId}`}
                          className="text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300 font-medium"
                        >
                          {entry.receiptNumber}
                        </Link>
                      ) : (
                        <span className="text-gray-600 dark:text-gray-400">
                          {entry.paymentMethod 
                            ? `Оплата (${entry.paymentMethod === 'cash' ? 'готівка' : entry.paymentMethod === 'card' ? 'картка' : entry.paymentMethod === 'transfer' ? 'переказ' : entry.paymentMethod})`
                            : 'Оплата'}
                        </span>
                      )}
                    </td>
                    <td className="py-2 text-gray-500 dark:text-gray-400">
                      {formatDate(entry.date)}
                    </td>
                    <td className="py-2 text-right text-gray-900 dark:text-white">
                      {entry.type === 'receipt' ? formatCurrency(entry.totalAmount!) : '-'}
                    </td>
                    <td className="py-2 text-right text-gray-900 dark:text-white">
                      {entry.type === 'receipt' ? formatCurrency(entry.paidAmount!) : formatCurrency(entry.paymentAmount!)}
                    </td>
                    <td className="py-2 text-right text-danger-600 font-medium">
                      {entry.type === 'receipt' ? formatCurrency(entry.debtAmount!) : '0.00'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
};

export default DebtorCard;
