import React, { useState, useEffect } from 'react';
import { Users, Search, Loader2, X, UserPlus, Phone, DollarSign, CreditCard, ArrowLeft, CheckCircle } from 'lucide-react';
import { debtorService, Debtor } from '@/services/debtorService';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { formatCurrency } from '@/utils/format';
import toast from 'react-hot-toast';

const DebtorsPage: React.FC = () => {
  const [debtors, setDebtors] = useState<Debtor[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newDebtorName, setNewDebtorName] = useState('');
  const [newDebtorPhone, setNewDebtorPhone] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  // Pay debt modal
  const [selectedDebtor, setSelectedDebtor] = useState<Debtor | null>(null);
  const [payAmount, setPayAmount] = useState('');
  const [isPaying, setIsPaying] = useState(false);

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
      const updated = await debtorService.payDebt(selectedDebtor.id, { amount });
      if (updated.total_debt <= 0) {
        toast.success(`Борг ${formatCurrency(amount)} сплачено повністю. Боржника видалено`);
      } else {
        toast.success(`Оплачено ${formatCurrency(amount)}. Залишок боргу: ${formatCurrency(updated.total_debt)}`);
      }
      setSelectedDebtor(null);
      setPayAmount('');
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
        <div className="grid gap-4">
          {filteredDebtors.map((debtor) => (
            <div
              key={debtor.id}
              className="card p-5 flex items-center justify-between hover:border-primary-300 dark:hover:border-primary-600 transition-all"
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-full bg-primary-100 dark:bg-primary-900/30 flex items-center justify-center flex-shrink-0">
                    <span className="text-primary-700 dark:text-primary-400 font-bold text-lg">
                      {debtor.name.charAt(0).toUpperCase()}
                    </span>
                  </div>
                  <div>
                    <h3 className="font-semibold text-gray-900 dark:text-gray-100">
                      {debtor.name}
                    </h3>
                    {debtor.phone && (
                      <p className="text-sm text-gray-500 flex items-center gap-1 mt-0.5">
                        <Phone className="w-3 h-3" />
                        {debtor.phone}
                      </p>
                    )}
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-6">
                <div className="text-right">
                  <p className="text-xs text-gray-400">Поточний борг</p>
                  <p className={`text-xl font-bold ${debtor.total_debt > 0 ? 'text-danger-600' : 'text-success-600'}`}>
                    {debtor.total_debt > 0 ? formatCurrency(debtor.total_debt) : '0.00 грн'}
                  </p>
                </div>
                {debtor.total_debt > 0 && (
                  <Button
                    variant="secondary"
                    onClick={() => {
                      setSelectedDebtor(debtor);
                      setPayAmount('');
                    }}
                  >
                    <CreditCard className="w-4 h-4 mr-2" />
                    Сплатити
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

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
        }}
        title={`Сплата боргу — ${selectedDebtor?.name || ''}`}
        size="sm"
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

            <div className="flex justify-end gap-3 pt-2">
              <Button
                variant="secondary"
                onClick={() => {
                  setSelectedDebtor(null);
                  setPayAmount('');
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
