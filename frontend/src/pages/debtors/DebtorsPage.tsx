import React, { useState, useEffect } from 'react';
import {Users, Search, Loader2, X, UserPlus, Phone, DollarSign, CreditCard, CheckCircle, Banknote} from 'lucide-react';
import { debtorService, Debtor, DebtorPayment } from '@/services/debtorService';
import { Receipt } from '@/types/receipt';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { formatCurrency } from '@/utils/format';
import DebtorListItem from './DebtorListItem';
import DebtorCard from './DebtorCard';
import toast from 'react-hot-toast';

const DebtorsPage: React.FC = () => {
  const [debtors, setDebtors] = useState<Debtor[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newDebtorName, setNewDebtorName] = useState('');
  const [newDebtorPhone, setNewDebtorPhone] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  // Debtor card modal state
  const [selectedDebtorForCard, setSelectedDebtorForCard] = useState<Debtor | null>(null);
  const [selectedDebtorReceipts, setSelectedDebtorReceipts] = useState<Receipt[]>([]);
  const [selectedDebtorPayments, setSelectedDebtorPayments] = useState<DebtorPayment[]>([]);
  const [isCardModalOpen, setIsCardModalOpen] = useState(false);
  const [isLoadingReceipts, setIsLoadingReceipts] = useState(false);

type DebtPaymentMethod = 'cash' | 'card' | 'transfer' | 'mixed';

const debtPaymentOptions: { value: DebtPaymentMethod; label: string; icon: React.ReactNode }[] = [
  { value: 'cash', label: 'Готівка', icon: <Banknote className="w-5 h-5" /> },
  { value: 'card', label: 'Картка', icon: <CreditCard className="w-5 h-5" /> },
  { value: 'mixed', label: 'Змішаний', icon: <CreditCard className="w-5 h-5" /> },
];

  // Pay debt modal
  const [selectedDebtor, setSelectedDebtor] = useState<Debtor | null>(null);
  const [payAmount, setPayAmount] = useState('');
  const [isPaying, setIsPaying] = useState(false);
  const [debtPaymentMethod, setDebtPaymentMethod] = useState<DebtPaymentMethod>('cash');
  const [debtCashAmount, setDebtCashAmount] = useState('');
  const [debtCardAmount, setDebtCardAmount] = useState('');

  const loadDebtors = async () => {
    setIsLoading(true);
    try {
      const data = searchQuery.trim()
        ? await debtorService.search(searchQuery)
        : await debtorService.list();
      setDebtors(data);
    } catch {
      toast.error('Помилка завантаження списку боржників');
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    loadDebtors();
  }, []);

  // Search with debounce
  useEffect(() => {
    const timer = setTimeout(() => {
      loadDebtors();
    }, 300);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  const handleDebtorClick = async (debtor: Debtor) => {
    setSelectedDebtorForCard(debtor);
    setIsCardModalOpen(true);
    setIsLoadingReceipts(true);
    try {
      const [receipts, payments] = await Promise.all([
        debtorService.getDebtorReceipts(debtor.id),
        debtorService.getDebtorPayments(debtor.id),
      ]);
      setSelectedDebtorReceipts(receipts);
      setSelectedDebtorPayments(payments);
    } catch {
      setSelectedDebtorReceipts([]);
      setSelectedDebtorPayments([]);
    } finally {
      setIsLoadingReceipts(false);
    }
  };

  const handleCreateDebtor = async () => {
    if (!newDebtorName.trim()) {
      toast.error('Введіть ім\'я боржника');
      return;
    }
    setIsCreating(true);
    try {
      await debtorService.create({
        name: newDebtorName.trim(),
        phone: newDebtorPhone.trim() || undefined,
      });
      toast.success(`Боржника "${newDebtorName.trim()}" створено`);
      setShowCreateModal(false);
      setNewDebtorName('');
      setNewDebtorPhone('');
      loadDebtors();
    } catch {
      toast.error('Помилка створення боржника');
    } finally {
      setIsCreating(false);
    }
  };

  const handlePayDebt = async () => {
    if (!selectedDebtor) return;
    const amount = parseFloat(payAmount);
    if (!amount || amount <= 0) {
      toast.error('Введіть коректну суму');
      return;
    }
    if (amount > selectedDebtor.total_debt) {
      toast.error(`Сума перевищує борг (${formatCurrency(selectedDebtor.total_debt)})`);
      return;
    }
    setIsPaying(true);
    try {
      const methodLabel = debtPaymentOptions.find(o => o.value === debtPaymentMethod)?.label || debtPaymentMethod;
      const updated = await debtorService.payDebt(selectedDebtor.id, { 
        amount,
        payment_method: debtPaymentMethod 
      });
      if (updated.total_debt <= 0) {
        toast.success(`Борг ${formatCurrency(amount)} сплачено повністю (${methodLabel}). Боржника видалено`);
      } else {
        toast.success(`Оплачено ${formatCurrency(amount)} (${methodLabel}). Залишок боргу: ${formatCurrency(updated.total_debt)}`);
      }
      setSelectedDebtor(null);
      setPayAmount('');
      setDebtPaymentMethod('cash');
      setDebtCashAmount('');
      setDebtCardAmount('');
      loadDebtors();
    } catch {
      toast.error('Помилка оплати боргу');
    } finally {
      setIsPaying(false);
    }
  };

  const filteredDebtors = debtors; // вже відфільтровано на бекенді або через search

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Users className="w-6 h-6 text-primary-600" />
          <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100">
            Боржники
          </h1>
        </div>
        <Button onClick={() => setShowCreateModal(true)}>
          <UserPlus className="w-4 h-4 mr-2" />
          Додати боржника
        </Button>
      </div>

      {/* Search */}
      <div className="card p-4">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400 pointer-events-none" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Пошук боржників за ім'ям..."
            className="input-field pl-10 pr-10"
            id="debtor-search"
            name="debtor-search"
            autoComplete="off"
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery('')}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* Debtors list */}
      {isLoading ? (
        <div className="flex items-center justify-center py-20">
          <Loader2 className="w-8 h-8 text-primary-500 animate-spin" />
        </div>
      ) : filteredDebtors.length === 0 ? (
        <div className="card p-12 text-center">
          <Users className="w-16 h-16 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
          <p className="text-gray-500 dark:text-gray-400 text-lg">
            {searchQuery ? 'Боржників за запитом не знайдено' : 'Список боржників порожній'}
          </p>
          {!searchQuery && (
            <Button
              variant="secondary"
              className="mt-4"
              onClick={() => setShowCreateModal(true)}
            >
              <UserPlus className="w-4 h-4 mr-2" />
              Додати першого боржника
            </Button>
          )}
        </div>
      ) : (
        <div className="space-y-3">
          {filteredDebtors.map((debtor) => (
            <DebtorListItem
              key={debtor.id}
              debtor={debtor}
              onClick={handleDebtorClick}
            />
          ))}
        </div>
      )}

      {/* Debtor card modal */}
      <Modal
        isOpen={isCardModalOpen}
        onClose={() => {
          setIsCardModalOpen(false);
          setSelectedDebtorForCard(null);
          setSelectedDebtorReceipts([]);
          setSelectedDebtorPayments([]);
        }}
        title={selectedDebtorForCard?.name || 'Картка боржника'}
        size="4xl"
      >
        {isLoadingReceipts ? (
          <div className="flex justify-center py-10">
            <Loader2 className="w-8 h-8 text-primary-500 animate-spin" />
          </div>
        ) : selectedDebtorForCard ? (
          <DebtorCard
            debtor={selectedDebtorForCard}
            receipts={selectedDebtorReceipts}
            payments={selectedDebtorPayments}
            onPay={(debtor) => {
              setIsCardModalOpen(false);
              setSelectedDebtor(debtor);
            }}
          />
        ) : null}
      </Modal>

      {/* Create debtor modal */}
      <Modal
        isOpen={showCreateModal}
        onClose={() => {
          setShowCreateModal(false);
          setNewDebtorName('');
          setNewDebtorPhone('');
        }}
        title="Новий боржник"
        size="sm"
      >
        <div className="space-y-4">
          <Input
            label="Ім'я боржника"
            value={newDebtorName}
            onChange={(e) => setNewDebtorName(e.target.value)}
            placeholder="Введіть ім'я"
            autoFocus
            id="new-debtor-name"
            name="new-debtor-name"
          />
          <Input
            label="Телефон (необов'язково)"
            value={newDebtorPhone}
            onChange={(e) => setNewDebtorPhone(e.target.value)}
            placeholder="+380..."
            icon={<Phone className="w-4 h-4" />}
            id="new-debtor-phone"
            name="new-debtor-phone"
          />
          <div className="flex justify-end gap-3 pt-2">
            <Button
              variant="secondary"
              onClick={() => {
                setShowCreateModal(false);
                setNewDebtorName('');
                setNewDebtorPhone('');
              }}
            >
              Скасувати
            </Button>
            <Button
              onClick={handleCreateDebtor}
              isLoading={isCreating}
            >
              Створити
            </Button>
          </div>
        </div>
      </Modal>

      {/* Pay debt modal */}
      <Modal
        isOpen={!!selectedDebtor}
        onClose={() => {
          setSelectedDebtor(null);
          setPayAmount('');
          setDebtPaymentMethod('cash');
          setDebtCashAmount('');
          setDebtCardAmount('');
        }}
        title={`Сплата боргу — ${selectedDebtor?.name || ''}`}
        size="md"
      >
        {selectedDebtor && (
          <div className="space-y-4">
            <div className="text-center p-4 bg-gray-50 dark:bg-slate-700/50 rounded-lg">
              <p className="text-sm text-gray-500">Поточний борг</p>
              <p className="text-3xl font-bold text-danger-600">
                {formatCurrency(selectedDebtor.total_debt)}
              </p>
            </div>

            <Input
              label="Сума для сплати"
              type="number"
              step="0.01"
              min="0.01"
              max={selectedDebtor.total_debt}
              value={payAmount}
              onChange={(e) => setPayAmount(e.target.value)}
              placeholder="Введіть суму"
              icon={<DollarSign className="w-4 h-4" />}
              autoFocus
              id="pay-amount"
              name="pay-amount"
            />

            {payAmount && parseFloat(payAmount) > 0 && (
              <div className="flex justify-between items-center p-3 bg-primary-50 dark:bg-primary-900/20 rounded-lg">
                <span className="text-sm text-gray-600 dark:text-gray-400">
                  Залишиться боргу
                </span>
                <span className="text-lg font-bold text-gray-900 dark:text-gray-100">
                  {formatCurrency(Math.max(0, selectedDebtor.total_debt - parseFloat(payAmount)))}
                </span>
              </div>
            )}

            {/* Спосіб оплати */}
            <div className="border-t border-gray-200 dark:border-slate-700 pt-4">
              <p className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
                Спосіб оплати
              </p>
              <div className="flex flex-wrap gap-2">
                {debtPaymentOptions.map((option) => (
                  <button
                    key={option.value}
                    onClick={() => setDebtPaymentMethod(option.value)}
                    className={`
                      flex items-center gap-2 px-4 py-2.5 rounded-lg border text-sm font-medium transition-all
                      ${debtPaymentMethod === option.value
                        ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/20 text-primary-700 dark:text-primary-400 ring-1 ring-primary-500'
                        : 'border-gray-200 dark:border-slate-600 bg-white dark:bg-slate-700 text-gray-600 dark:text-gray-300 hover:border-gray-300 dark:hover:border-slate-500'
                      }
                    `}
                  >
                    {option.icon}
                    {option.label}
                  </button>
                ))}
              </div>
              {debtPaymentMethod === 'mixed' && (
                <div className="grid grid-cols-2 gap-4 mt-3">
                  <div>
                    <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Готівка</label>
                    <input
                      type="number"
                      step="0.01"
                      min="0"
                      value={debtCashAmount}
                      onChange={(e) => {
                        const val = e.target.value;
                        setDebtCashAmount(val);
                        const cash = parseFloat(val) || 0;
                        const debtAmount = selectedDebtor.total_debt;
                        const remaining = debtAmount - cash;
                        setDebtCardAmount(remaining >= 0 ? remaining.toFixed(2) : '0');
                      }}
                      className="input-field"
                      placeholder="0.00"
                    />
                  </div>
                  <div>
                    <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Картка</label>
                    <input
                      type="number"
                      step="0.01"
                      min="0"
                      value={debtCardAmount}
                      onChange={(e) => {
                        const val = e.target.value;
                        setDebtCardAmount(val);
                        const card = parseFloat(val) || 0;
                        const debtAmount = selectedDebtor.total_debt;
                        const remaining = debtAmount - card;
                        setDebtCashAmount(remaining >= 0 ? remaining.toFixed(2) : '0');
                      }}
                      className="input-field"
                      placeholder="0.00"
                    />
                  </div>
                </div>
              )}
            </div>

            <div className="flex justify-end gap-3 pt-2">
              <Button
                variant="secondary"
                onClick={() => {
                  setSelectedDebtor(null);
                  setPayAmount('');
                  setDebtPaymentMethod('cash');
                  setDebtCashAmount('');
                  setDebtCardAmount('');
                }}
              >
                Скасувати
              </Button>
              <Button
                onClick={handlePayDebt}
                isLoading={isPaying}
                disabled={!payAmount || parseFloat(payAmount) <= 0}
              >
                <CheckCircle className="w-4 h-4 mr-2" />
                Сплатити
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
};

export default DebtorsPage;
